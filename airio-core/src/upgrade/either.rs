use crate::{Upgrade, UpgradeInfo, either::EitherFuture};

use either::Either;
use futures::future;
use std::iter::Map;

impl<A, B> UpgradeInfo for Either<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo,
{
    type Info = Either<A::Info, B::Info>;
    type InfoIter = Either<
        Map<<A::InfoIter as IntoIterator>::IntoIter, fn(A::Info) -> Self::Info>,
        Map<<B::InfoIter as IntoIterator>::IntoIter, fn(B::Info) -> Self::Info>,
    >;

    fn protocol_info(&self) -> Self::InfoIter {
        match self {
            Either::Left(a) => Either::Left(a.protocol_info().into_iter().map(Either::Left)),
            Either::Right(b) => Either::Right(b.protocol_info().into_iter().map(Either::Right)),
        }
    }
}

impl<C, A, B, TA, TB, EA, EB> Upgrade<C> for Either<A, B>
where
    A: Upgrade<C, Output = TA, Error = EA>,
    B: Upgrade<C, Output = TB, Error = EB>,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_inbound(self, stream: C, info: Self::Info) -> Self::Future {
        match (self, info) {
            (Either::Left(a), Either::Left(info)) => {
                EitherFuture::Left(a.upgrade_inbound(stream, info))
            }
            (Either::Right(b), Either::Right(info)) => {
                EitherFuture::Right(b.upgrade_inbound(stream, info))
            }
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_inbound"),
        }
    }

    fn upgrade_outbound(self, stream: C, info: Self::Info) -> Self::Future {
        match (self, info) {
            (Either::Left(a), Either::Left(info)) => {
                EitherFuture::Left(a.upgrade_outbound(stream, info))
            }
            (Either::Right(b), Either::Right(info)) => {
                EitherFuture::Right(b.upgrade_outbound(stream, info))
            }
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_outbound"),
        }
    }
}
