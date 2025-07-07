use std::{
    pin::Pin,
    task::{Context, Poll},
};

use either::Either;
use futures::{TryFuture, future};
use pin_project::pin_project;

#[pin_project(project = EitherFutureProj)]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture<A, B> {
    Left(#[pin] A),
    Right(#[pin] B),
}

impl<AFuture, BFuture, AInner, BInner> Future for EitherFuture<AFuture, BFuture>
where
    AFuture: TryFuture<Ok = AInner>,
    BFuture: TryFuture<Ok = BInner>,
{
    type Output = Result<future::Either<AInner, BInner>, Either<AFuture::Error, BFuture::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherFutureProj::Left(a) => a
                .try_poll(cx)
                .map_ok(future::Either::Left)
                .map_err(Either::Left),
            EitherFutureProj::Right(a) => a
                .try_poll(cx)
                .map_ok(future::Either::Right)
                .map_err(Either::Right),
        }
    }
}
