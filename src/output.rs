use gio::prelude::*;
use std::io;

pub(crate) struct StreamWriter(pub gio::WriteOutputStream);

impl io::Write for StreamWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(self
            .0
            .write(buf, gio::NONE_CANCELLABLE)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))? as usize)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(self
            .0
            .flush(gio::NONE_CANCELLABLE)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?)
    }
}
