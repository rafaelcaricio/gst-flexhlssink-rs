use gio::prelude::*;
use glib::subclass::prelude::*;
use gst::prelude::*;
use gst::subclass::prelude::*;
use gst::{gst_debug, gst_error, gst_info, gst_trace, gst_warning};

use crate::playlist::PlaylistRenderState;
use m3u8_rs::playlist::{MediaPlaylist, MediaPlaylistType, MediaSegment};
use once_cell::sync::Lazy;
use std::fs;
use std::path;
use std::sync::{Arc, Mutex};

const DEFAULT_LOCATION: &str = "segment%05d.ts";
const DEFAULT_PLAYLIST_LOCATION: &str = "playlist.m3u8";
const DEFAULT_MAX_NUM_SEGMENT_FILES: u32 = 10;
const DEFAULT_TARGET_DURATION: u32 = 15;
const DEFAULT_PLAYLIST_LENGTH: u32 = 5;
const DEFAULT_SEND_KEYFRAME_REQUESTS: bool = true;

const GST_M3U8_PLAYLIST_VERSION: usize = 3;
const BACKWARDS_COMPATIBLE_PLACEHOLDER: &str = "%05d";

static CAT: Lazy<gst::DebugCategory> = Lazy::new(|| {
    gst::DebugCategory::new(
        "flexhlssink",
        gst::DebugColorFlags::empty(),
        Some("Flexible HLS sink"),
    )
});

struct Settings {
    location: String,
    playlist_location: String, // TODO: Evaluate the use of `PathBuf` instead.
    playlist_root: Option<String>, // TODO: Evaluate the use of `PathBuf` instead.
    playlist_length: u32,
    max_num_segment_files: usize,
    target_duration: u32,
    send_keyframe_requests: bool,

    // TODO: old_locations ? Maybe just use another thread and send msgs with files to delete ?
    splitmuxsink: Option<gst::Element>,
    giostreamsink: Option<gst::Element>,
    video_sink: bool,
    audio_sink: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            location: String::from(DEFAULT_LOCATION),
            playlist_location: String::from(DEFAULT_PLAYLIST_LOCATION),
            playlist_root: None,
            playlist_length: DEFAULT_PLAYLIST_LENGTH,
            max_num_segment_files: DEFAULT_MAX_NUM_SEGMENT_FILES as usize,
            target_duration: DEFAULT_TARGET_DURATION,
            send_keyframe_requests: DEFAULT_SEND_KEYFRAME_REQUESTS,

            splitmuxsink: None,
            giostreamsink: None,
            video_sink: false,
            audio_sink: false,
        }
    }
}

enum State {
    Stopped,
    Started {
        playlist: MediaPlaylist,
        playlist_render_state: PlaylistRenderState,
        playlist_index: usize,

        fragment_opened_at: Option<gst::ClockTime>,
        current_segment_location: Option<String>,
        old_segment_locations: Vec<String>,
    },
}

impl Default for State {
    fn default() -> Self {
        Self::Stopped
    }
}

#[derive(Default, Clone)]
pub struct FlexHlsSink {
    settings: Arc<Mutex<Settings>>,
    state: Arc<Mutex<State>>,
}

impl FlexHlsSink {
    fn new() -> Self {
        Self {
            settings: Arc::new(Mutex::new(Settings::default())),
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    fn start(
        &self,
        element: &super::FlexHlsSink,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        gst_info!(CAT, obj: element, "Starting");

        let settings = self.settings.lock().unwrap();
        let target_duration = settings.target_duration as f32;

        let mut state = self.state.lock().unwrap();
        if let State::Stopped = *state {
            *state = State::Started {
                playlist: MediaPlaylist {
                    version: GST_M3U8_PLAYLIST_VERSION,
                    target_duration,
                    media_sequence: 0,
                    segments: vec![],
                    discontinuity_sequence: 0,
                    end_list: false,
                    playlist_type: Some(MediaPlaylistType::Vod),
                    i_frames_only: false,
                    start: None,
                    independent_segments: false,
                    unknown_tags: vec![],
                },
                playlist_render_state: PlaylistRenderState::Init,
                playlist_index: 0,
                current_segment_location: None,
                fragment_opened_at: None,
                old_segment_locations: Vec::new(),
            };
        }

        Ok(gst::StateChangeSuccess::Success)
    }

    fn on_format_location(
        &self,
        element: &super::FlexHlsSink,
        fragment_id: u32,
    ) -> Result<String, String> {
        gst_info!(
            CAT,
            "Starting the formatting of the fragment-id: {}",
            fragment_id
        );

        let mut state = self.state.lock().unwrap();
        let current_segment_location = match &mut *state {
            State::Stopped => return Err("Not in Started state".to_string()),
            State::Started {
                current_segment_location,
                ..
            } => current_segment_location,
        };

        let settings = self.settings.lock().unwrap();

        let seq_num = format!("{:0>5}", fragment_id);
        let segment_file_location = settings
            .location
            .replace(BACKWARDS_COMPATIBLE_PLACEHOLDER, &seq_num);
        gst_trace!(CAT, "Segment location formatted: {}", segment_file_location);

        *current_segment_location = Some(segment_file_location.clone());

        // TODO: this should be a call to the signal exposed by this plugin
        let stream = self
            .new_file_stream(element, &segment_file_location)
            .map_err(|err| err.to_string())?;
        let giostreamsink = settings.giostreamsink.as_ref().unwrap();
        giostreamsink.set_property("stream", &stream).unwrap();

        gst_info!(
            CAT,
            "New segment location: {}",
            current_segment_location.as_ref().unwrap()
        );
        Ok(segment_file_location)
    }

    fn new_file_stream<P>(
        &self,
        element: &super::FlexHlsSink,
        location: &P,
    ) -> Result<gio::OutputStream, String>
    where
        P: AsRef<path::Path>,
    {
        let element_weak = element.downgrade();
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(location)
            .map_err(move |err| {
                let error_msg = gst::error_msg!(
                    gst::ResourceError::OpenWrite,
                    [
                        "Could not open file {} for writing: {}",
                        location.as_ref().to_str().unwrap(),
                        err.to_string(),
                    ]
                );
                let element = element_weak.upgrade().unwrap();
                element.post_error_message(error_msg);
                err.to_string()
            })?;
        Ok(gio::WriteOutputStream::new(file).upcast())
    }

    fn write_playlist(
        &self,
        element: &super::FlexHlsSink,
        fragment_closed_at: gst::ClockTime,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        gst_info!(CAT, obj: element, "Preparing to write new playlist");

        let mut state = self.state.lock().unwrap();
        match &mut *state {
            State::Stopped => {}
            State::Started {
                fragment_opened_at,
                playlist,
                current_segment_location,
                playlist_render_state,
                playlist_index,
                old_segment_locations,
                ..
            } => {
                gst_info!(CAT, "COUNT {}", playlist.segments.len());
                // TODO: Add new entry to the playlist

                let segment_location = current_segment_location
                    .take()
                    .ok_or_else(|| gst::StateChangeError)?;

                playlist.segments.push(MediaSegment {
                    uri: segment_location.clone(),
                    duration: {
                        let fragment_opened_at =
                            fragment_opened_at.as_ref().ok_or(gst::StateChangeError)?;

                        let segment_duration = fragment_closed_at - fragment_opened_at;

                        segment_duration.seconds().ok_or(gst::StateChangeError)? as f32
                    },
                    title: None,
                    byte_range: None,
                    discontinuity: false,
                    key: None,
                    map: None,
                    program_date_time: None,
                    daterange: None,
                });

                let (playlist_location, max_num_segments, max_playlist_length) = {
                    let settings = self.settings.lock().unwrap();
                    (
                        settings.playlist_location.clone(),
                        settings.max_num_segment_files,
                        settings.playlist_length as usize,
                    )
                };

                // TODO: remove old segments from playlist
                if playlist.segments.len() > max_playlist_length {
                    for _ in 0..playlist.segments.len() - max_playlist_length {
                        let _ = playlist.segments.remove(0);
                    }
                }

                *playlist_index += 1;
                playlist.media_sequence = *playlist_index as i32 - playlist.segments.len() as i32;

                // TODO: this should be a call to the signal exposed by this plugin
                let mut playlist_file = self
                    .new_file_stream(&element, &playlist_location)
                    .map_err(|_| gst::StateChangeError)?
                    .into_write();

                playlist.write_to(&mut playlist_file).map_err(|err| {
                    gst_error!(
                        CAT,
                        "Could not write new playlist file: {}",
                        err.to_string()
                    );
                    gst::StateChangeError
                })?;

                *playlist_render_state = PlaylistRenderState::Started;

                old_segment_locations.push(segment_location);
                if old_segment_locations.len() > max_num_segments {
                    for _ in 0..old_segment_locations.len() - max_num_segments {
                        let old_segment_location = old_segment_locations.remove(0);
                        // TODO: trigger event to delete segment location
                        let _ = fs::remove_file(&old_segment_location).map_err(|err| {
                            gst_warning!(CAT, "Could not delete segment file: {}", err.to_string());
                        });
                    }
                }
            }
        };

        gst_debug!(CAT, obj: element, "Wrote new playlist file!");
        Ok(gst::StateChangeSuccess::Success)
    }

    fn write_final_playlist(
        &self,
        element: &super::FlexHlsSink,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        gst_debug!(CAT, obj: element, "Preparing to write final playlist");
        Ok(self.write_playlist(element, element.current_running_time())?)
    }

    fn stop(&self, element: &super::FlexHlsSink) {
        gst_debug!(CAT, obj: element, "Stopping");

        let mut state = self.state.lock().unwrap();
        if let State::Started { .. } = *state {
            *state = State::Stopped;
        }

        gst_debug!(CAT, obj: element, "Stopped");
    }
}

#[glib::object_subclass]
impl ObjectSubclass for FlexHlsSink {
    const NAME: &'static str = "FlexHlsSink";
    type Type = super::FlexHlsSink;
    type ParentType = gst::Bin;

    fn with_class(_klass: &Self::Class) -> Self {
        Self::new()
    }
}

impl BinImpl for FlexHlsSink {
    #[allow(clippy::single_match)]
    fn handle_message(&self, element: &Self::Type, msg: gst::Message) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Element(ref msg) => {
                let event_is_from_splitmuxsink = {
                    let settings = self.settings.lock().unwrap();

                    settings.splitmuxsink.is_some()
                        && msg.src().as_ref()
                            == Some(settings.splitmuxsink.as_ref().unwrap().upcast_ref())
                };

                if event_is_from_splitmuxsink {
                    let s = msg.structure().unwrap();
                    match s.name() {
                        "splitmuxsink-fragment-opened" => {
                            if let Ok(new_fragment_opened_at) =
                                s.get::<gst::ClockTime>("running-time")
                            {
                                let mut state = self.state.lock().unwrap();
                                match &mut *state {
                                    State::Stopped => return,
                                    State::Started {
                                        fragment_opened_at, ..
                                    } => *fragment_opened_at = Some(new_fragment_opened_at),
                                };
                            }
                        }
                        "splitmuxsink-fragment-closed" => {
                            let s = msg.structure().unwrap();
                            if let Ok(fragment_closed_at) = s.get::<gst::ClockTime>("running-time")
                            {
                                self.write_playlist(element, fragment_closed_at).unwrap();
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => self.parent_handle_message(element, msg),
        }
    }
}

impl ObjectImpl for FlexHlsSink {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
            vec![
                glib::ParamSpec::new_string(
                    "location",
                    "File Location",
                    "Location of the file to write",
                    Some(DEFAULT_LOCATION),
                    glib::ParamFlags::READWRITE,
                ),
                glib::ParamSpec::new_string(
                    "playlist-location",
                    "Playlist Location",
                    "Location of the playlist to write.",
                    Some(DEFAULT_PLAYLIST_LOCATION),
                    glib::ParamFlags::READWRITE,
                ),
                glib::ParamSpec::new_string(
                    "playlist-root",
                    "Playlist Root",
                    "Location of the playlist to write.",
                    None,
                    glib::ParamFlags::READWRITE,
                ),
                glib::ParamSpec::new_uint(
                    "max-files",
                    "Max files",
                    "Maximum number of files to keep on disk. Once the maximum is reached, old files start to be deleted to make room for new ones.",
                    0,
                    u32::MAX,
                    DEFAULT_MAX_NUM_SEGMENT_FILES,
                    glib::ParamFlags::READWRITE,
                ),
                glib::ParamSpec::new_uint(
                    "target-duration",
                    "Target duration",
                    "The target duration in seconds of a segment/file. (0 - disabled, useful for management of segment duration by the streaming server)",
                    0,
                    u32::MAX,
                    DEFAULT_TARGET_DURATION,
                    glib::ParamFlags::READWRITE,
                ),
                glib::ParamSpec::new_uint(
                    "playlist-length",
                    "Playlist length",
                    "Length of HLS playlist. To allow players to conform to section 6.3.3 of the HLS specification, this should be at least 3. If set to 0, the playlist will be infinite.",
                    0,
                    u32::MAX,
                    DEFAULT_PLAYLIST_LENGTH,
                    glib::ParamFlags::READWRITE,
                ),
                glib::ParamSpec::new_boolean(
                    "send-keyframe-requests",
                    "Send Keyframe Requests",
                    "Send keyframe requests to ensure correct fragmentation. If this is disabled then the input must have keyframes in regular intervals.",
                    DEFAULT_SEND_KEYFRAME_REQUESTS,
                    glib::ParamFlags::READWRITE,
                ),
            ]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(
        &self,
        _obj: &Self::Type,
        _id: usize,
        value: &glib::Value,
        pspec: &glib::ParamSpec,
    ) {
        let mut settings = self.settings.lock().unwrap();
        match pspec.name() {
            "location" => {
                settings.location = value
                    .get::<Option<String>>()
                    .expect("type checked upstream")
                    .unwrap_or_else(|| DEFAULT_LOCATION.into());
                if let Some(splitmuxsink) = &settings.splitmuxsink {
                    splitmuxsink
                        .set_property("location", &settings.location)
                        .unwrap();
                }
            }
            "playlist-location" => {
                settings.playlist_location = value
                    .get::<Option<String>>()
                    .expect("type checked upstream")
                    .unwrap_or_else(|| DEFAULT_LOCATION.into());
            }
            "playlist-root" => {
                settings.playlist_root = value
                    .get::<Option<String>>()
                    .expect("type checked upstream");
            }
            "max-files" => {
                let max_files: u32 = value.get().expect("type checked upstream");
                settings.max_num_segment_files = max_files as usize;
            }
            "target-duration" => {
                settings.target_duration = value.get().expect("type checked upstream");
                if let Some(splitmuxsink) = &settings.splitmuxsink {
                    splitmuxsink
                        .set_property(
                            "max-size-time",
                            &((settings.target_duration as u64) * gst::SECOND_VAL),
                        )
                        .unwrap();
                }
            }
            "playlist-length" => {
                settings.playlist_length = value.get().expect("type checked upstream");
            }
            "send-keyframe-requests" => {
                settings.send_keyframe_requests = value.get().expect("type checked upstream");
                if let Some(splitmuxsink) = &settings.splitmuxsink {
                    splitmuxsink
                        .set_property("send-keyframe-requests", &settings.send_keyframe_requests)
                        .unwrap();
                }
            }
            _ => unimplemented!(),
        };
    }

    fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        let settings = self.settings.lock().unwrap();
        match pspec.name() {
            "location" => settings.location.to_value(),
            "playlist-location" => settings.playlist_location.to_value(),
            "playlist-root" => settings.playlist_root.to_value(),
            "max-files" => {
                let max_files = settings.max_num_segment_files as u32;
                max_files.to_value()
            }
            "target-duration" => settings.target_duration.to_value(),
            "playlist-length" => settings.playlist_length.to_value(),
            "send-keyframe-requests" => settings.send_keyframe_requests.to_value(),
            _ => unimplemented!(),
        }
    }

    // Called right after construction of a new instance
    fn constructed(&self, obj: &Self::Type) {
        // Call the parent class' ::constructed() implementation first
        self.parent_constructed(obj);

        let mut settings = self.settings.lock().unwrap();

        let splitmuxsink = gst::ElementFactory::make("splitmuxsink", Some("split_mux_sink"))
            .expect("Could not make element splitmuxsink");
        let giostreamsink = gst::ElementFactory::make("giostreamsink", Some("giostream_sink"))
            .expect("Could not make element giostreamsink");

        let mux = gst::ElementFactory::make("mpegtsmux", Some("mpeg-ts_mux"))
            .expect("Could not make element mpegtsmux");

        let location: Option<String> = None;
        splitmuxsink
            .set_properties(&[
                ("location", &location),
                (
                    "max-size-time",
                    &((settings.target_duration as u64) * gst::SECOND_VAL),
                ),
                ("send-keyframe-requests", &true),
                ("muxer", &mux),
                ("sink", &giostreamsink),
                ("reset-muxer", &false),
            ])
            .unwrap();

        obj.set_element_flags(gst::ElementFlags::SINK);
        obj.add(&splitmuxsink).unwrap();

        let this = self.clone();
        let element_weak = obj.downgrade();
        splitmuxsink
            .connect("format-location", false, move |args| {
                let fragment_id = args[1].get::<u32>().unwrap();

                gst_info!(CAT, "Got fragment-id: {}", fragment_id);

                let element = element_weak.upgrade().unwrap();
                match this.on_format_location(&element, fragment_id) {
                    Ok(segment_location) => Some(segment_location.to_value()),
                    Err(err) => {
                        gst_error!(CAT, "on format-location handler: {}", err);
                        Some("unknown_segment".to_value())
                    }
                }
            })
            .unwrap();

        settings.splitmuxsink = Some(splitmuxsink);
        settings.giostreamsink = Some(giostreamsink);
    }
}

impl ElementImpl for FlexHlsSink {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: Lazy<gst::subclass::ElementMetadata> = Lazy::new(|| {
            gst::subclass::ElementMetadata::new(
                "Flexible HTTP Live Streaming sink",
                "Sink/Muxer",
                "Flexible HTTP Live Streaming sink",
                "Alessandro Decina <alessandro.d@gmail.com>, \
                Sebastian Dröge <sebastian@centricular.com>, \
                Rafael Caricio <rafael@caricio.com>",
            )
        });

        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: Lazy<Vec<gst::PadTemplate>> = Lazy::new(|| {
            let caps = gst::Caps::new_any();
            let video_pad_template = gst::PadTemplate::new(
                "video",
                gst::PadDirection::Sink,
                gst::PadPresence::Request,
                &caps,
            )
            .unwrap();

            let caps = gst::Caps::new_any();
            let audio_pad_template = gst::PadTemplate::new(
                "audio",
                gst::PadDirection::Sink,
                gst::PadPresence::Request,
                &caps,
            )
            .unwrap();

            vec![video_pad_template, audio_pad_template]
        });

        PAD_TEMPLATES.as_ref()
    }

    fn change_state(
        &self,
        element: &Self::Type,
        transition: gst::StateChange,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        match transition {
            gst::StateChange::NullToReady => {
                self.start(element)?;
            }
            _ => (),
        }

        let ret = self.parent_change_state(element, transition)?;

        match transition {
            gst::StateChange::PausedToReady => {
                // Turning down
                let write_final = {
                    let mut state = self.state.lock().unwrap();
                    match &mut *state {
                        State::Stopped => false,
                        State::Started {
                            playlist,
                            playlist_render_state,
                            ..
                        } => {
                            if *playlist_render_state == PlaylistRenderState::Started {
                                playlist.end_list = true;
                                true
                            } else {
                                false
                            }
                        }
                    }
                };

                if write_final {
                    self.write_final_playlist(element)?;
                }
            }
            gst::StateChange::ReadyToNull => {
                self.stop(element);
            }
            _ => (),
        }

        Ok(ret)
    }

    fn request_new_pad(
        &self,
        element: &Self::Type,
        templ: &gst::PadTemplate,
        _name: Option<String>,
        _caps: Option<&gst::Caps>,
    ) -> Option<gst::Pad> {
        let mut settings = self.settings.lock().unwrap();
        match templ.name_template().as_ref().map(|val| val.as_str()) {
            Some("audio") => {
                if settings.audio_sink {
                    gst_debug!(
                        CAT,
                        obj: element,
                        "requested_new_pad: audio pad is already set"
                    );
                    return None;
                }

                let splitmuxsink = match &mut settings.splitmuxsink {
                    None => return None,
                    Some(sms) => sms,
                };
                let peer_pad = splitmuxsink.request_pad_simple("audio_0").unwrap();
                let sink_pad =
                    gst::GhostPad::from_template_with_target(&templ, Some("audio"), &peer_pad)
                        .unwrap();
                element.add_pad(&sink_pad).unwrap();
                sink_pad.set_active(true).unwrap();
                settings.audio_sink = true;

                Some(sink_pad.upcast())
            }
            Some("video") => {
                if settings.video_sink {
                    gst_debug!(
                        CAT,
                        obj: element,
                        "requested_new_pad: video pad is already set"
                    );
                    return None;
                }
                let splitmuxsink = match &mut settings.splitmuxsink {
                    None => return None,
                    Some(sms) => sms,
                };
                let peer_pad = splitmuxsink.request_pad_simple("video").unwrap();

                let sink_pad =
                    gst::GhostPad::from_template_with_target(&templ, Some("video"), &peer_pad)
                        .unwrap();
                element.add_pad(&sink_pad).unwrap();
                sink_pad.set_active(true).unwrap();
                settings.video_sink = true;

                Some(sink_pad.upcast())
            }
            None => {
                gst_debug!(CAT, obj: element, "template name returned `None`",);
                None
            }
            Some(other_name) => {
                gst_debug!(
                    CAT,
                    obj: element,
                    "requested_new_pad: name \"{}\" is not audio or video",
                    other_name
                );
                None
            }
        }
    }

    fn release_pad(&self, element: &Self::Type, pad: &gst::Pad) {
        let mut settings = self.settings.lock().unwrap();

        if !settings.audio_sink && !settings.video_sink {
            return;
        }

        let ghost_pad = pad.downcast_ref::<gst::GhostPad>().unwrap();
        if let Some(peer) = ghost_pad.target() {
            settings
                .splitmuxsink
                .as_ref()
                .unwrap()
                .release_request_pad(&peer);
        }

        pad.set_active(false).unwrap();
        element.remove_pad(pad).unwrap();

        if "audio" == ghost_pad.name() {
            settings.audio_sink = false;
        } else {
            settings.video_sink = false;
        }
    }
}
