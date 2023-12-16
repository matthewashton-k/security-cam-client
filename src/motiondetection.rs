use std::error::Error;
use security_cam_common::shuttle_runtime::tokio::sync::mpsc::{channel, Receiver, Sender};
use security_cam_common::shuttle_runtime::tokio::task;
use security_cam_common::shuttle_runtime::tokio::task::JoinHandle;

/// used for connecting to /dev/video0 and reading in frames to detect any motion
pub struct MotionDetector<'a> {
    /// filenames are sent through this channel
    tx: Sender<String>,

    /// filenames received through this channel
    rx: Receiver<String>,

    /// path to the video device, eg /dev/video0
    video_device: &'a str,

    motion_detection_thread: Option<JoinHandle<()>>,
}

impl<'a> MotionDetector<'a> {
    pub fn new(video_device: &'a str) -> Self {
        let (tx, rx) = channel(50);
        Self {
            tx,
            rx,
            video_device,
            motion_detection_thread: None,
        }
    }

    /// if there is a new motion capture saved, this function will return its file path, if not, it will return None
    pub async fn try_ask_for_filename(&mut self) -> Option<String> {
        self.rx.try_recv().ok()
    }

    pub fn start_detection(&mut self) -> Result<(), Box<dyn Error>> {
        if self.motion_detection_thread.is_some() {
            return Err("already started".into());
        }
        self.motion_detection_thread = Some(task::spawn(async {

        }));
        Ok(())
    }

    pub fn stop_detection(&mut self) {
        todo!("do this");
    }
}