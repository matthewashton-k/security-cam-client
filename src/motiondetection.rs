use std::error::Error;
use std::slice::Chunks;
use std::sync::Arc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use image::{GenericImage, GenericImageView, GrayImage, ImageBuffer, ImageFormat, Luma, Pixel, Rgb, Rgba};
use imageproc::contrast::threshold;
use imageproc::utils::{Diff};
use nokhwa::{Buffer, CallbackCamera, Camera, NokhwaError};
use nokhwa::pixel_format::{LumaFormat, RgbAFormat, RgbFormat};
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};

const THRESHOLD_VALUE: i32 = 60;
#[derive(Clone,Debug)]
pub enum FileCommand {
    Error(String),
    FileName(String)
}


/// used for connecting to /dev/video0 and reading in frames to detect any motion
pub struct MotionDetector {
    /// filenames are sent through this channel
    tx: Sender<FileCommand>,

    /// filenames received through this channel
    rx: Receiver<FileCommand>,

    /// path to the video device, eg /dev/video0
    video_device: u32,

    pub motion_detection_thread: Option<JoinHandle<()>>,

    ///when to stop recording after processing frames
    buffer_delay: Duration
}

impl MotionDetector {
    pub fn new(video_device: u32) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            video_device,
            motion_detection_thread: None,
            buffer_delay: Duration::from_secs(5)
        }
    }

    /// if there is a new motion capture saved, this function will return its file path, if not, it will return None
    pub fn ask_for_filename(&mut self) -> Option<FileCommand> {
        self.rx.recv().ok()
    }

    pub  fn start_detection(&mut self) -> Result<(), Box<dyn Error>> {
        if self.motion_detection_thread.is_some() {
            return Err("already started".into());
        }
        let index = CameraIndex::Index(self.video_device);
        // request the absolute highest resolution CameraFormat that can be decoded to RGB.
        let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);

        // how often to take a snapshot to compare future frames with
        let tx = self.tx.clone();

        self.motion_detection_thread = Some(thread::spawn( move || {
            let mut threaded = CallbackCamera::new(index, requested, |buffer| {
            }).unwrap();
            threaded.open_stream().unwrap();
            let mut frame1:Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;
            let mut frame2: Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;
            let mut frame3: Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;
            let mut last_movement: Option<Instant> = None;
            let mut framecounter = 0;
            let mut videocounter = 0;
            loop {

                let buffer = match threaded.poll_frame() {
                    Ok(buf) => {
                        buf
                    }
                    Err(e) => {
                        println!("there was an error");
                        tx.send(FileCommand::Error(e.to_string()));
                        continue;
                    }
                };
                match buffer.decode_image::<LumaFormat>() {
                    Ok(frame) => {
                        // Shift the frames
                        frame1 = frame2;
                        frame2 = frame3;
                        frame3 = Some(frame);
                        if let (Some(f1), Some(f2), Some(f3)) = (&frame1, &frame2, &frame3) {
                            // Calculate the difference between f2 and f1, and between f3 and f2
                            let diff1 = pixel_diffs(f2, f1, |(x1, y1, p1), (x2, y2, p2)| {
                                (p1[0].abs_diff(p2[0])) > 30
                            });

                            let diff2 = pixel_diffs(f3, f2, |(x1, y1, p1), (x2, y2, p2)| {
                                (p1[0].abs_diff(p2[0])) > 30
                            });

                            let diff1= diffs_to_gray_image(diff1, f3.width(),f3.height());
                            let diff2 = diffs_to_gray_image(diff2, f3.width(),f3.height());
                            // Threshold the differences
                            let thresholded_diff1 = threshold(&diff1, THRESHOLD_VALUE as u8);
                            let thresholded_diff2 = threshold(&diff2, THRESHOLD_VALUE as u8);

                            // Combine the differences with a logical AND
                            let score = movement_score(&thresholded_diff1, &thresholded_diff2);

                            if let Some(time) = last_movement {
                                let time = time.elapsed().as_secs();
                                if time < 10 {
                                    let mut filename = "video_frames/".to_string();
                                    filename.push_str(&videocounter.to_string());
                                    filename.push_str(".");
                                    filename.push_str(&framecounter.to_string());
                                    filename.push_str(".jpg");
                                    buffer.decode_image::<RgbFormat>().unwrap().save(filename).unwrap();
                                    framecounter += 1;
                                } else {
                                    tx.send(FileCommand::FileName(videocounter.to_string()));
                                    last_movement = None;
                                    videocounter +=1;
                                }
                            }

                            if score > 5 {
                                last_movement = Some(Instant::now());
                                println!("movement detected");
                            }
                            // Now 'result' contains the detected object without its ghost
                        }
                    }
                    Err(e) => {
                        tx.send(FileCommand::Error(e.to_string()));
                    }
                }
            }
        }));
        Ok(())
    }

}

pub fn pixel_diffs<I, J, F, P>(actual: &I, expected: &J, is_diff: F) -> Vec<Diff<I::Pixel>>
    where
        P: Pixel,
        I: GenericImage<Pixel = P>,
        J: GenericImage<Pixel = P>,
        F: Fn((u32, u32, I::Pixel), (u32, u32, J::Pixel)) -> bool,
{
    let (width, height) = actual.dimensions();
    assert_eq!((width, height), expected.dimensions(), "Image dimensions do not match");

    let mut diffs = Vec::new();

    for y in 0..height {
        for x in 0..width {
            let p = (x, y, actual.get_pixel(x, y));
            let q = (x, y, expected.get_pixel(x, y));

            if is_diff(p, q) {
                diffs.push(Diff {
                    x: p.0,
                    y: p.1,
                    actual: p.2,
                    expected: q.2,
                });
            }
        }
    }

    diffs
}



fn diffs_to_gray_image(diffs: Vec<Diff<Luma<u8>>>, width: u32, height: u32) -> GrayImage {
    // Convert each Diff<Rgb<u8>> to a grayscale pixel
    let mut grey_image:GrayImage = ImageBuffer::new(width, height);
    // let gray_pixel = Luma::from([(diff.actual.to_luma()[0].abs_diff(diff.expected.to_luma()[0]))]);
    // Construct a GrayImage from the grayscale pixels
    for diff in diffs {
        grey_image.put_pixel(diff.x,diff.y, Luma::from([(diff.actual.to_luma()[0].abs_diff(diff.expected.to_luma()[0]))]));
    }

    grey_image
}

fn movement_score(image1: &GrayImage, image2: &GrayImage) -> u32 {
    let mut count = 0;
    for (x, y, pixel) in image1.enumerate_pixels() {
        let pixel2 = image2.get_pixel(x, y);
        if (pixel[0] & pixel2[0]) != 0 {
            count +=1;
        }
    }
    return count;
}