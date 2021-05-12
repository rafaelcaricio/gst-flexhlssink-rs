use glib::prelude::*;

mod imp;
mod playlist;

glib::wrapper! {
    pub struct FlexHlsSink(ObjectSubclass<imp::FlexHlsSink>) @extends gst::Bin, gst::Element, gst::Object;
}

unsafe impl Send for FlexHlsSink {}
unsafe impl Sync for FlexHlsSink {}

pub fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "flexhlssink",
        gst::Rank::None,
        FlexHlsSink::static_type(),
    )?;

    Ok(())
}

gst::plugin_define!(
    flexhlssink,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    "MIT/X11",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
