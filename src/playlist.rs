use m3u8_rs::playlist;

pub struct MediaPlaylist(playlist::MediaPlaylist);

impl MediaPlaylist {
    fn inner_mut(&mut self) -> &mut playlist::MediaPlaylist {
        &mut self.0
    }

    fn inner(&self) -> &playlist::MediaPlaylist {
        &self.0
    }
}

pub enum PlaylistRenderState {
    Init,
    Started,
    Ended,
}
