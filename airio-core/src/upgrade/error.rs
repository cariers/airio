use airio_stream_select::NegotiationError;

#[derive(Debug, thiserror::Error)]
pub enum UpgradeError<E> {
    #[error(transparent)]
    Select(#[from] NegotiationError),
    #[error(transparent)]
    Apply(E),
}
