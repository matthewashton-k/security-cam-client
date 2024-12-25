use std::error::Error;
use std::fs::File;
use std::io::{Cursor, Seek, Write};

use image::codecs::jpeg::JpegDecoder;
use image::{DynamicImage, GenericImage, GenericImageView, GrayImage, ImageBuffer, Luma, Pixel};
use imageproc::contrast::threshold;
use imageproc::utils::Diff;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use v4l::buffer::Type;
use v4l::io::traits::CaptureStream;
use v4l::io::userptr::Stream;
use v4l::prelude::UserptrStream;
use v4l::video::Capture;
use v4l::Device;
use v4l::FourCC;

const THRESHOLD_VALUE: i32 = 60;

#[derive(Debug, Clone)]
pub struct Frame {
    pub video_num: usize,
    pub frame_bytes: Vec<u8>,
    pub fps: usize,
    pub end: bool,
}

/// Error contains any error message thrown during the frame reading loop
/// Frame range indicates that there are new frames in /video_frames
/// frames in video_frames have the format {video num}.{frame_num}.jpg
/// if two files have the same video num, then they should be in the same video.
/// frame num is the frame number in the video, where the second number in FrameRange is the last
/// frame number.
#[derive(Clone, Debug)]
pub enum FrameCommand {
    Error(String),

    /// video_num, frame_num (count), frame_rate
    FrameRange(usize, u64, usize),

    SingleFrame(Frame),
}

/// used for connecting to /dev/video0 and reading in frames to detect any motion
pub struct MotionDetector {
    /// filenames are sent through this channel
    tx: Sender<FrameCommand>,

    /// filenames received through this channel
    rx: Receiver<FrameCommand>,

    /// path to the video device, eg /dev/video0
    video_device: u32,

    pub motion_detection_thread: Option<JoinHandle<()>>,

    ///when to stop recording after processing frames
    buffer_delay: Duration,
}

impl MotionDetector {
    pub fn new(video_device: u32) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            video_device,
            motion_detection_thread: None,
            buffer_delay: Duration::from_secs(5),
        }
    }

    /// if there is a new motion capture saved, this function will return its file path, if not, it will return None
    pub fn ask_for_filename(&mut self) -> Option<FrameCommand> {
        self.rx.recv().ok()
    }

    pub fn start_detection(&mut self) -> Result<(), Box<dyn Error>> {
        if self.motion_detection_thread.is_some() {
            return Err("already started".into());
        }
        let mut device = Device::new(self.video_device as usize)?;
        let mut format = device.format()?;
        format.fourcc = FourCC::new(b"MJPG");
        format = device.set_format(&format)?;
        println!("{:?}", format);
        let params = device.params()?;
        let mut stream = UserptrStream::new(&device, Type::VideoCapture)?;

        // send FileCommands through tx to interact with the main thread
        let tx = self.tx.clone();

        // the diffs of f1 and f2, and f2 and f3 are used to see if motion is detece
        let mut frame1: Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;
        let mut frame2: Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;
        let mut frame3: Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;

        // time since movement was last detected, or None if movement hasnt been detected for 10 seconds
        let mut last_movement: Option<Instant> = None;
        let mut framecounter = 0;
        let mut videocounter = 0;

        let mut framerate_time = Instant::now();
        let mut framerate_counter = 0;
        let mut fps = 25;
        self.motion_detection_thread = Some(thread::spawn(move || {
            println!("started thread");
            // ----------------------------------------------------------------
            // -------------------FRAME PROCESSING LOOP -----------------------
            // ----------------------------------------------------------------
            loop {
                let buf = if let Ok((buf, _)) = stream.next() {
                    buf
                } else {
                    tx.send(FrameCommand::Error("failed to capture frame".to_string()))
                        .expect("failed to send error");
                    continue;
                };
                match decode(buf) {
                    Ok(frame_dynamic) => {
                        let frame = frame_dynamic.to_luma8();
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

                            let diff1 = diffs_to_gray_image(diff1, f3.width(), f3.height());
                            let diff2 = diffs_to_gray_image(diff2, f3.width(), f3.height());
                            // Threshold the differences
                            let thresholded_diff1 = threshold(
                                &diff1,
                                THRESHOLD_VALUE as u8,
                                imageproc::contrast::ThresholdType::Binary,
                            );
                            let thresholded_diff2 = threshold(
                                &diff2,
                                THRESHOLD_VALUE as u8,
                                imageproc::contrast::ThresholdType::Binary,
                            );

                            // Combine the differences with a logical AND
                            let score = movement_score(&thresholded_diff1, &thresholded_diff2);
                            if let Some(time) = last_movement {
                                let time = time.elapsed().as_secs();
                                if time < 10 {
                                    // if movement is still going on
                                    let filename =
                                        gen_filename(&mut framecounter, &mut videocounter);
                                    tx.send(FrameCommand::SingleFrame(Frame {
                                        video_num: videocounter,
                                        frame_bytes: buf.to_vec(),
                                        fps,
                                        end: false,
                                    }))
                                    .expect("failed to send frame");
                                    framecounter += 1;
                                } else {
                                    // if movement has previously been detected but stopped, then its time to send a clip
                                    // tx.send(FrameCommand::FrameRange(
                                    //     videocounter,
                                    //     framecounter,
                                    //     fps,
                                    // ));
                                    tx.send(FrameCommand::SingleFrame(Frame {
                                        video_num: videocounter,
                                        frame_bytes: buf.to_vec(),
                                        fps: fps,
                                        end: true,
                                    }))
                                    .expect("failed to send frame");
                                    last_movement = None;
                                    videocounter += 1;
                                    framecounter = 0;
                                }
                            }

                            if score > 5 {
                                println!("movement detected!");
                                last_movement = Some(Instant::now());
                            }
                        }
                    }
                    Err(e) => {
                        tx.send(FrameCommand::Error(e.to_string()))
                            .expect("failed to send frame error");
                    }
                }

                framerate_counter += 1;
                // Calculate frame rate every second
                if framerate_time.elapsed().as_secs() >= 1 {
                    fps = framerate_counter;
                    println!("FPS: {}", fps);
                    // Reset counter and timer
                    framerate_counter = 0;
                    framerate_time = Instant::now();
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
    assert_eq!(
        (width, height),
        expected.dimensions(),
        "Image dimensions do not match"
    );

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

/// helper method for making a filename from a frame counter and a video counter
fn gen_filename(framecounter: &u64, videocounter: &usize) -> String {
    let mut filename = "video_frames/".to_string();
    filename.push_str(&videocounter.to_string());
    filename.push_str(".");
    filename.push_str(&framecounter.to_string());
    filename.push_str(".jpg");
    filename
}

fn diffs_to_gray_image(diffs: Vec<Diff<Luma<u8>>>, width: u32, height: u32) -> GrayImage {
    // Convert each Diff<Rgb<u8>> to a grayscale pixel
    let mut grey_image: GrayImage = ImageBuffer::new(width, height);
    // let gray_pixel = Luma::from([(diff.actual.to_luma()[0].abs_diff(diff.expected.to_luma()[0]))]);
    // Construct a GrayImage from the grayscale pixels
    for diff in diffs {
        grey_image.put_pixel(
            diff.x,
            diff.y,
            Luma::from([(diff.actual.to_luma()[0].abs_diff(diff.expected.to_luma()[0]))]),
        );
    }

    grey_image
}

/// decodes a buffer into a dynamicimage
fn decode(jpg: &[u8]) -> Result<DynamicImage, Box<dyn Error>> {
    let decoder = JpegDecoder::new(Cursor::new(jpg))?;
    Ok(DynamicImage::from_decoder(decoder)?)
}

fn movement_score(image1: &GrayImage, image2: &GrayImage) -> u32 {
    let mut count = 0;
    for (x, y, pixel) in image1.enumerate_pixels() {
        let pixel2 = image2.get_pixel(x, y);
        if (pixel[0] & pixel2[0]) != 0 {
            count += 1;
        }
    }
    return count;
}
