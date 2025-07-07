use std::{
    mem,
    pin::Pin,
    task::{Context, Poll},
};

use airio_stream_select::{DialerSelectFuture, ListenerSelectFuture};
use futures::{AsyncRead, AsyncWrite};

use crate::{Negotiated, Upgrade, upgrade::UpgradeError};

enum UpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    ListenerInit {
        future: ListenerSelectFuture<C, U::Info>,
        upgrade: U,
    },

    DialerInit {
        future: DialerSelectFuture<C, <U::InfoIter as IntoIterator>::IntoIter>,
        upgrade: U,
    },

    Upgrade {
        future: Pin<Box<U::Future>>,
        name: String,
    },
    Undefined,
}

pub struct UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    inner: UpgradeApplyState<C, U>,
}

impl<C, U> UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    pub fn new_outbound(io: C, upgrade: U) -> Self {
        UpgradeApply {
            inner: UpgradeApplyState::DialerInit {
                future: DialerSelectFuture::new(io, upgrade.protocol_info()),
                upgrade,
            },
        }
    }

    pub fn new_inbound(io: C, upgrade: U) -> Self {
        UpgradeApply {
            inner: UpgradeApplyState::ListenerInit {
                future: ListenerSelectFuture::new(io, upgrade.protocol_info()),
                upgrade,
            },
        }
    }
}

impl<C, U> Unpin for UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
}

impl<C, U> Future for UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    type Output = Result<U::Output, UpgradeError<U::Error>>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match mem::replace(&mut self.inner, UpgradeApplyState::Undefined) {
                UpgradeApplyState::DialerInit {
                    mut future,
                    upgrade,
                } => {
                    // Poll 底层的协商结果
                    let (info, connection) = match Pin::new(&mut future).poll(cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = UpgradeApplyState::DialerInit { future, upgrade };
                            return Poll::Pending;
                        }
                    };
                    // 协商成功，开始升级
                    self.inner = UpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_outbound(connection, info.clone())),
                        name: info.as_ref().to_owned(),
                    };
                }
                UpgradeApplyState::ListenerInit {
                    mut future,
                    upgrade,
                } => {
                    // Poll Listener协商结果
                    tracing::trace!("Negotiating inbound connection");

                    let (info, connection) = match Pin::new(&mut future).poll(cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = UpgradeApplyState::ListenerInit { future, upgrade };
                            return Poll::Pending;
                        }
                    };
                    tracing::trace!(upgrade=%info.as_ref(), "Negotiated inbound connection");
                    // 协商成功，开始升级
                    self.inner = UpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_inbound(connection, info.clone())),
                        name: info.as_ref().to_owned(),
                    };
                }
                UpgradeApplyState::Upgrade { mut future, name } => {
                    // Poll 升级结果
                    match Pin::new(&mut future).poll(cx) {
                        Poll::Ready(Ok(x)) => {
                            tracing::trace!(upgrade=%name, "Upgraded stream");
                            return Poll::Ready(Ok(x));
                        }
                        Poll::Ready(Err(e)) => {
                            tracing::debug!(upgrade=%name, "Failed to upgrade stream",);
                            return Poll::Ready(Err(UpgradeError::Apply(e)));
                        }
                        Poll::Pending => {
                            self.inner = UpgradeApplyState::Upgrade { future, name };
                            return Poll::Pending;
                        }
                    }
                }

                _ => panic!("Invalid state in UpgradeApply"),
            }
        }
    }
}
