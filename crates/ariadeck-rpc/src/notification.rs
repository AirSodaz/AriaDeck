use ariadeck_domain::Gid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Aria2NotificationKind {
    DownloadStarted,
    DownloadPaused,
    DownloadStopped,
    DownloadCompleted,
    DownloadErrored,
    BitTorrentDownloadCompleted,
    Unknown(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Aria2Notification {
    pub kind: Aria2NotificationKind,
    pub gid: Option<Gid>,
}
