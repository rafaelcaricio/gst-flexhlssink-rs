use glib::prelude::*;
use gst::gst_info;
use gst::prelude::*;
use gst_base::prelude::*;
use once_cell::sync::Lazy;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
fn test_basic_element_with_video_content() {
    init();

    const BUFFER_NB: i32 = 600;

    let pipeline = gst::Pipeline::new(Some("video_pipeline"));

    let video_src = gst::ElementFactory::make("videotestsrc", Some("test_videotestsrc")).unwrap();
    video_src.set_property("is-live", &true).unwrap();

    let x264enc = gst::ElementFactory::make("x264enc", Some("test_x264enc")).unwrap();
    let h264parse = gst::ElementFactory::make("h264parse", Some("test_h264parse")).unwrap();

    let tee = gst::ElementFactory::make("tee", Some("test_tee")).unwrap();

    let hls_queue = gst::ElementFactory::make("queue", Some("test_hls_queue")).unwrap();
    let flexhlssink = gst::ElementFactory::make("flexhlssink", Some("test_flexhlssink")).unwrap();
    flexhlssink.set_property("target-duration", &6u32).unwrap();

    let app_queue = gst::ElementFactory::make("queue", Some("test_app_queue")).unwrap();
    let app_sink = gst::ElementFactory::make("appsink", Some("test_sink")).unwrap();
    app_sink.set_property("sync", &false).unwrap();
    app_sink.set_property("async", &false).unwrap();

    pipeline
        .add_many(&[
            &video_src,
            &x264enc,
            &h264parse,
            &tee,
            &app_queue,
            &app_sink,
            &hls_queue,
            &flexhlssink,
        ])
        .unwrap();

    gst::Element::link_many(&[&video_src, &x264enc, &h264parse, &tee]).unwrap();

    gst::Element::link_many(&[&app_queue, &app_sink]).unwrap();
    gst::Element::link_many(&[&hls_queue, &flexhlssink]).unwrap();

    // Link the appsink
    let tee_app_pad = tee.request_pad_simple("src_%u").unwrap();
    let app_queue_pad = app_queue.static_pad("sink").unwrap();
    tee_app_pad.link(&app_queue_pad).unwrap();

    // Link the flexhlssink branch
    let tee_hls_pad = tee.request_pad_simple("src_%u").unwrap();
    let hls_queue_pad = hls_queue.static_pad("sink").unwrap();
    tee_hls_pad.link(&hls_queue_pad).unwrap();

    let appsink = app_sink.dynamic_cast::<gst_app::AppSink>().unwrap();
    appsink.set_emit_signals(true);
    let (sender, receiver) = mpsc::channel();
    appsink.connect_new_sample(move |appsink| {
        let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
        let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;

        //gst_info!(CAT, "TEST sample buffer[{}]", buffer.size());

        sender.send(()).unwrap();
        Ok(gst::FlowSuccess::Ok)
    });

    pipeline.set_state(gst::State::Playing).unwrap();

    gst_info!(
        CAT,
        "flexhlssink_video_pipeline: waiting for {} buffers",
        BUFFER_NB
    );
    for idx in 0..BUFFER_NB {
        receiver.recv().unwrap();
        //gst_info!(CAT, "flexhlssink_video_pipeline: received buffer #{}", idx);
    }

    pipeline.set_state(gst::State::Null).unwrap();
}

#[test]
fn test_basic_element_properties() {
    init();

    const BUFFER_NB: i32 = 3;

    let pipeline = gst::Pipeline::new(Some("audio_pipeline"));

    let audio_src = gst::ElementFactory::make("audiotestsrc", Some("audiotestsrc")).unwrap();
    audio_src.set_property("is-live", &true).unwrap();
    audio_src.set_property("num-buffers", &BUFFER_NB).unwrap();

    let tee = gst::ElementFactory::make("tee", Some("tee")).unwrap();

    let hls_queue = gst::ElementFactory::make("queue", Some("hls_queue")).unwrap();
    let hls_avenc_aac = gst::ElementFactory::make("avenc_aac", Some("hls_avenc_aac")).unwrap();
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
            &hls_avenc_aac,
            &flexhlssink,
        ])
        .unwrap();

    gst::Element::link_many(&[&audio_src, &tee]).unwrap();
    gst::Element::link_many(&[&app_queue, &app_sink]).unwrap();
    gst::Element::link_many(&[&hls_queue, &hls_avenc_aac, &flexhlssink]).unwrap();

    // Link the appsink
    let tee_app_pad = tee.request_pad_simple("src_%u").unwrap();
    let app_queue_pad = app_queue.static_pad("sink").unwrap();
    tee_app_pad.link(&app_queue_pad).unwrap();

    // Link the flexhlssink branch
    let tee_hls_pad = tee.request_pad_simple("src_%u").unwrap();
    let hls_queue_pad = hls_queue.static_pad("sink").unwrap();
    tee_hls_pad.link(&hls_queue_pad).unwrap();

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

    gst_info!(CAT, "audio_pipeline: waiting for {} buffers", BUFFER_NB);
    for idx in 0..BUFFER_NB {
        receiver.recv().unwrap();
        gst_info!(CAT, "audio_pipeline: received buffer #{}", idx);
    }

    pipeline.set_state(gst::State::Null).unwrap();
}
