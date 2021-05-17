use m3u8_rs::playlist;

pub struct MediaPlaylist(playlist::MediaPlaylist);

impl MediaPlaylist {}

#[derive(Copy, Clone, PartialEq)]
pub enum PlaylistRenderState {
    Init,
    Started,
}
