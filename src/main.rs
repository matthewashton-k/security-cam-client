// TODO

use security_cam_client::motiondetection::{FrameCommand, MotionDetector};
use security_cam_client::networking::Client;
use security_cam_common::shuttle_runtime::tokio;
use std::error::Error;
use std::fs::{create_dir, DirEntry};
use std::future::Future;
use std::path::Path;

#[actix_web::main]
async fn main() {
    // username, passcode, and address should be read in from the command line and then a new Client can be constructed from them
    set_up_dirs().expect("couldnt create video_frames directory");
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 6 {
        println!(
            "Usage: client <username> <passcode> <address> <video device> <cooldown duration>"
        );
        return;
    }

    let username = &args[1];
    let passcode = &args[2];
    let address = &args[3].trim();
    let cooldown_duration = &args[5]
        .parse::<usize>()
        .expect("cooldown time must be an integer");
    let video_device: &u32 = &args[4].parse().expect("video device must be an integer");
    println!("{address}");
    let mut client = Client::new(address, username, passcode).await;
    client.login().await.expect("failed to login");
    let mut motion_detector = MotionDetector::new(*video_device, *cooldown_duration);

    // start detection loop
    motion_detector
        .start_detection()
        .expect("failed to start detection");
    while let Some(command) = motion_detector.ask_for_filename() {
        match command {
            FrameCommand::Error(e) => {
                eprintln!("[ERROR] error in camera capture stream: {}", e);
            }
            FrameCommand::FrameRange(video_num, last_frame_num, fps) => {
                println!("{last_frame_num} = last frame num");
                match client
                    .send_frame_range(video_num, last_frame_num as usize + 1, fps)
                    .await
                {
                    Ok(_) => {
                        println!("sent frame range");
                    }
                    Err(e) => {
                        eprintln!("[ERROR]: {e:?}"); // Propagate the error
                    }
                }
            }
            FrameCommand::SingleFrame(frame) => {
                let is_last_frame = frame.end;
                match client.send_frame_framereader(frame).await {
                    Ok(_) => {
                        println!("sent frame");
                    }
                    Err(e) => {
                        client.tx = None;
                        if let Some(task) = client.transfer_task {
                            task.abort();
                        }
                        client.transfer_task = None;
                        eprintln!("[ERROR]: {e:?}"); // Propagate the error
                    }
                }
                if is_last_frame {
                    println!("Processing last frame");
                    if let Some(tx) = client.tx.take() {
                        drop(tx);
                    }
                    if let Some(task) = client.transfer_task.take() {
                        // Give the task time to complete
                        match tokio::time::timeout(std::time::Duration::from_secs(10), task).await {
                            Ok(Ok(_)) => println!("Transfer task completed successfully"),
                            Ok(Err(e)) => eprintln!("Transfer task failed: {:?}", e),
                            Err(_) => eprintln!("Transfer task timed out"),
                        }
                    }
                }
            }
        }
    }
}

fn set_up_dirs() -> Result<(), std::io::Error> {
    if Path::new("video_frames").exists() {
        return Ok(());
    } else {
        create_dir("video_frames")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use security_cam_client::motiondetection::MotionDetector;
    use security_cam_common::shuttle_runtime::tokio;

    #[tokio::test]
    async fn test_reqwest() {
        reqwest::get("https://httpbin.org/ip")
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_img_capture() {
        let mut motion_detector = MotionDetector::new(0, 10);
        motion_detector.start_detection().unwrap();
        motion_detector.motion_detection_thread.unwrap().join();
    }
}
