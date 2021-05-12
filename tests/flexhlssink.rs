use glib::prelude::*;
use gst::gst_info;
use gst::prelude::*;
use gst_base::prelude::*;
use once_cell::sync::Lazy;
use std::sync::mpsc;

static CAT: Lazy<gst::DebugCategory> = Lazy::new(|| {
    gst::DebugCategory::new(
        "flexhlssink-test",
        gst::DebugColorFlags::empty(),
        Some("Flex HLS sink test"),
    )
});

fn init() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        gst::init().unwrap();
        flexhlssink::plugin_register_static().expect("flexhlssink test");
    });
}

#[test]
fn test_basic_element_properties() {
    init();

    const BUFFER_NB: i32 = 3;

    let pipeline = gst::Pipeline::new(None);

    let audio_src = gst::ElementFactory::make("audiotestsrc", Some("audiotestsrc")).unwrap();
    audio_src.set_property("is-live", &true).unwrap();
    audio_src.set_property("num-buffers", &BUFFER_NB).unwrap();

    let decodebin = gst::ElementFactory::make("decodebin", Some("decodebin_base")).unwrap();

    let video_src = gst::ElementFactory::make("videotestsrc", Some("videotestsrc")).unwrap();
    video_src.set_property("is-live", &true).unwrap();
    video_src.set_property("num-buffers", &BUFFER_NB).unwrap();

    let audio_convert = gst::ElementFactory::make("audioconvert", Some("audioconvert")).unwrap();

    let tee = gst::ElementFactory::make("tee", Some("tee")).unwrap();

    let hls_queue = gst::ElementFactory::make("queue", Some("hls_queue")).unwrap();
    let hls_audio_convert = gst::ElementFactory::make("audioconvert", Some("hls_audioconvert")).unwrap();
    let flexhlssink = gst::ElementFactory::make("flexhlssink", Some("flexhlssink")).unwrap();
    flexhlssink.set_property("target-duration", &6u32).unwrap();

    let app_queue = gst::ElementFactory::make("queue", Some("app_queue")).unwrap();
    let app_sink = gst::ElementFactory::make("appsink", Some("appsink")).unwrap();
    app_sink.set_property("sync", &false).unwrap();
    app_sink.set_property("async", &false).unwrap();
    app_sink.set_property("emit-signals", &true).unwrap();

    pipeline
        .add_many(&[
            &audio_src,
            &tee,
            &app_queue,
            &app_sink,
            &hls_queue,
            &decodebin,
            &audio_convert,
            // &hls_audio_convert,
            &flexhlssink,
        ])
        .unwrap();

    gst::Element::link_many(&[&audio_src, &tee]).unwrap();
    gst::Element::link_many(&[&app_queue, &app_sink]).unwrap();
    gst::Element::link_many(&[&hls_queue, &decodebin]).unwrap();

    // hls_queue.link_pads(Some("src"), &hls_audio_convert, Some("sink")).unwrap();
    // audio_convert.link_pads(Some("src"), &flexhlssink, Some("audio")).unwrap();

    // Link the appsink
    let tee_app_pad = tee.request_pad_simple("src_%u").unwrap();
    println!(
        "Obtained request pad {} for the app branch",
        tee_app_pad.name()
    );
    let app_queue_pad = app_queue.static_pad("sink").unwrap();
    tee_app_pad.link(&app_queue_pad).unwrap();

    // Link the flexhlssink branch
    let tee_hls_pad = tee.request_pad_simple("src_%u").unwrap();
    println!(
        "Obtained request pad {} for the HLS branch",
        tee_hls_pad.name()
    );
    let hls_queue_pad = hls_queue.static_pad("sink").unwrap();
    tee_hls_pad.link(&hls_queue_pad).unwrap();

    // Link the queue to flexhlssink to link on audio
    // let audio_convert_pad = audio_convert.static_pad("src").unwrap();
    // println!(
    //     "Obtained request pad {} for the flex HLS sink",
    //     audio_convert_pad.name()
    // );
    // let hls_audio_pad = flexhlssink.request_pad_simple("audio").unwrap();
    // audio_convert_pad.link(&hls_audio_pad).unwrap();

    let audio_convert_clone = audio_convert.clone();
    let flexhlssink_clone = flexhlssink.clone();
    decodebin.connect_pad_added(move |_, pad| {
        let caps = pad.current_caps().unwrap();
        let s = caps.structure(0).unwrap();

        let audio_convert_sink_pad = audio_convert_clone.static_pad("sink").unwrap();

        if s.name() == "audio/x-raw" && !audio_convert_sink_pad.is_linked() {
            pad.link(&audio_convert_sink_pad).unwrap();

            let audio_convert_src_pad = audio_convert_clone.static_pad("src").unwrap();
            let hls_audio_pad = flexhlssink_clone.request_pad_simple("audio").unwrap();
            audio_convert_src_pad.link(&hls_audio_pad).unwrap();
        }
    });

    // audio_src.connect_pad_added(move |src, src_pad| {
    //     println!(
    //         "Received new pad {} from {}",
    //         src_pad.name(),
    //         src.name()
    //     );
    //     let tee_sink_pad = tee.request_pad_simple("sink").unwrap();
    //     if tee_sink_pad.is_linked() {
    //         println!("Already linked!");
    //         return;
    //     }
    //
    //     let new_pad_caps = src_pad
    //         .current_caps()
    //         .expect("Failed to get caps of new pad.");
    //     let new_pad_struct = new_pad_caps
    //         .structure(0)
    //         .expect("Failed to get first structure of caps.");
    //     let new_pad_type = new_pad_struct.name();
    //     let is_audio = new_pad_type.starts_with("audio/x-raw");
    //     if !is_audio {
    //         println!(
    //             "It has type {} which is not audio. Ignoring.",
    //             new_pad_type
    //         );
    //         return;
    //     }
    //
    //     src_pad.link(&tee_sink_pad).unwrap();
    // });

    // let audio_convert_pad = audio_convert.static_pad("src").unwrap();
    // println!(
    //     "Obtained request pad {} from the audioconvert",
    //     audio_convert_pad.name()
    // );
    // println!("Caps for new hls_audio_pad: {:?}", hls_audio_pad.allowed_caps());
    // audio_convert_pad.link(&hls_audio_pad).unwrap();

    let appsink = app_sink.dynamic_cast::<gst_app::AppSink>().unwrap();
    let (sender, receiver) = mpsc::channel();
    appsink.connect_new_sample(move |appsink| {
        let _sample = appsink
            .emit_by_name("pull-sample", &[])
            .unwrap()
            .unwrap()
            .get::<gst::Sample>()
            .unwrap();

        sender.send(()).unwrap();
        Ok(gst::FlowSuccess::Ok)
    });

    pipeline.set_state(gst::State::Playing).unwrap();

    gst_info!(
        CAT,
        "flexhlssink_pipeline: waiting for {} buffers",
        BUFFER_NB
    );
    for idx in 0..BUFFER_NB {
        receiver.recv().unwrap();
        gst_info!(CAT, "flexhlssink_pipeline: received buffer #{}", idx);
    }

    pipeline.set_state(gst::State::Null).unwrap();
}
