// TODO

use security_cam_client::motiondetection::{FileCommand, MotionDetector};
use security_cam_client::networking::Client;
use security_cam_common::shuttle_runtime::tokio;
use security_cam_client::ffmpeg::execute_ffmpeg;

#[tokio::main]
async fn main() {
    // username, passcode, and address should be read in from the command line and then a new Client can be constructed from them
    let args = std::env::args().collect::<Vec<_>>();
    if  args.len() != 5 {
        println!("Usage: client <username> <passcode> <address> <video device>");
        return;
    }
    let username = &args[1];
    let passcode = &args[2];
    let address = &args[3].trim();
    let video_device: &u32 = &args[4].parse().expect("video device must be an integer");
    println!("{address}");
    let mut client = Client::new(address, username, passcode).await;
    client.login().await.expect("failed to login");
    let mut motion_detector = MotionDetector::new(*video_device);

    // start detectino loop
    motion_detector.start_detection().expect("failed to start detection");
    while let Some(command) = motion_detector.ask_for_filename() {
         match command {
             FileCommand::Error(e) => {
                 println!("[ERROR] error in camera capture stream: {}", e);
             }
             FileCommand::FrameRange(video_num,last_frame_num) => {
                 match execute_ffmpeg(video_num,last_frame_num) {
                     Ok(filename) => {
                         match client.send_and_delete(filename).await {
                              Ok(_) => println!("Successfully sent file"),
                              Err(e) => println!("Error sending file: {}", e),
                         }
                     }
                     Err(e) => {
                         println!("[ERROR] {}",e.to_string());
                     }
                 }
             }
         }

    }
}


#[cfg(test)]
mod tests {
    use security_cam_common::shuttle_runtime::tokio;
    use security_cam_client::motiondetection::MotionDetector;

    #[tokio::test]
    async fn test_reqwest() {
        reqwest::get("https://httpbin.org/ip").await.unwrap().bytes().await.unwrap();
    }

    #[tokio::test]
    async fn test_img_capture() {
        let mut motion_detector = MotionDetector::new(0);
        motion_detector.start_detection().unwrap();
        motion_detector.motion_detection_thread.unwrap().join();
    }
}