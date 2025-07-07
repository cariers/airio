mod apply;
mod either;
mod error;
mod pending;
mod ready;
mod select;

pub use apply::UpgradeApply;
pub use error::UpgradeError;
pub use pending::PendingUpgrade;
pub use ready::ReadyUpgrade;
pub use select::SelectUpgrade;

pub trait UpgradeInfo {
    type Info: AsRef<str> + Clone;
    type InfoIter: Iterator<Item = Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter;
}

/// 流升级
pub trait Upgrade<S>: UpgradeInfo {
    type Output;
    type Error;
    type Future: Future<Output = Result<Self::Output, Self::Error>>;

    /// 升级入站流
    fn upgrade_inbound(self, stream: S, info: Self::Info) -> Self::Future;

    /// 升级出站流
    fn upgrade_outbound(self, stream: S, info: Self::Info) -> Self::Future;
}
