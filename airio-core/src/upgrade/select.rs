use std::iter::{Chain, Map};

use either::Either;
use futures::future;

use crate::{Upgrade, UpgradeInfo, either::EitherFuture};

#[derive(Debug, Clone)]
pub struct SelectUpgrade<A, B>(A, B);

impl<A, B> SelectUpgrade<A, B> {
    pub fn new(a: A, b: B) -> Self {
        SelectUpgrade(a, b)
    }
}

impl<A, B> UpgradeInfo for SelectUpgrade<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo,
{
    type Info = Either<A::Info, B::Info>;
    type InfoIter = Chain<
        Map<<A::InfoIter as IntoIterator>::IntoIter, fn(A::Info) -> Self::Info>,
        Map<<B::InfoIter as IntoIterator>::IntoIter, fn(B::Info) -> Self::Info>,
    >;

    fn protocol_info(&self) -> Self::InfoIter {
        let a = self
            .0
            .protocol_info()
            .into_iter()
            .map(Either::Left as fn(A::Info) -> _);
        let b = self
            .1
            .protocol_info()
            .into_iter()
            .map(Either::Right as fn(B::Info) -> _);

        a.chain(b)
    }
}

impl<C, A, B, TA, TB, EA, EB> Upgrade<C> for SelectUpgrade<A, B>
where
    A: Upgrade<C, Output = TA, Error = EA>,
    B: Upgrade<C, Output = TB, Error = EB>,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_inbound(self, stream: C, info: Self::Info) -> Self::Future {
        match info {
            Either::Left(info) => EitherFuture::Left(self.0.upgrade_inbound(stream, info)),
            Either::Right(info) => EitherFuture::Right(self.1.upgrade_inbound(stream, info)),
        }
    }

    fn upgrade_outbound(self, stream: C, info: Self::Info) -> Self::Future {
        match info {
            Either::Left(info) => EitherFuture::Left(self.0.upgrade_outbound(stream, info)),
            Either::Right(info) => EitherFuture::Right(self.1.upgrade_outbound(stream, info)),
        }
    }
}
