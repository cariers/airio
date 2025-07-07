use std::net::SocketAddr;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    Dialer,
    Listener,
}

impl std::ops::Not for Endpoint {
    type Output = Endpoint;

    fn not(self) -> Self::Output {
        match self {
            Endpoint::Dialer => Endpoint::Listener,
            Endpoint::Listener => Endpoint::Dialer,
        }
    }
}

impl Endpoint {
    pub fn is_dialer(self) -> bool {
        matches!(self, Endpoint::Dialer)
    }

    pub fn is_listener(self) -> bool {
        matches!(self, Endpoint::Listener)
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum ConnectedPoint {
    Dialer {
        addr: SocketAddr,
    },
    Listener {
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
    },
}

impl ConnectedPoint {
    pub fn is_dialer(&self) -> bool {
        matches!(self, ConnectedPoint::Dialer { .. })
    }

    pub fn is_listener(&self) -> bool {
        matches!(self, ConnectedPoint::Listener { .. })
    }
}
