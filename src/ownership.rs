use crate::core::contracts::Error;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::Path,
};

#[derive(Debug)]
pub struct Ownership {
    file: Option<File>,
}

impl Ownership {
    pub fn acquire(path: &Path) -> Result<Self, Error> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|_| unavailable())?;
        }
        if fs::symlink_metadata(path).is_ok_and(|m| crate::assets::is_link(&m) || !m.is_file()) {
            return Err(unavailable());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
            );
        }
        let file = options.open(path).map_err(|_| unavailable())?;
        if file
            .metadata()
            .map_or(true, |m| crate::assets::is_link(&m) || !m.is_file())
        {
            return Err(unavailable());
        }
        match file.try_lock() {
            Ok(()) => Ok(Self { file: Some(file) }),
            Err(TryLockError::WouldBlock) => Err(Error::new(
                "ownership_busy",
                "Another server owns this local task store.",
            )),
            Err(TryLockError::Error(_)) => Err(unavailable()),
        }
    }

    pub fn close(&mut self) {
        // Closing the handle also releases its OS lock after a crash; never unlink a live lock file.
        self.file.take();
    }
}
fn unavailable() -> Error {
    Error::new(
        "ownership_unavailable",
        "Local ownership lock could not be opened or acquired.",
    )
}
