// TODO

use security_cam_client::motiondetection::MotionDetector;
use security_cam_client::networking::Client;
use security_cam_common::shuttle_runtime::tokio;

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
    let address = &args[3];
    let video_device = &args[4];
    let mut client = Client::new(username, passcode, address);
    let mut motion_detector = MotionDetector::new(video_device);

    // start detectino loop
    motion_detector.start_detection().expect("failed to start detection");
    while let Some(filename) = motion_detector.try_ask_for_filename().await {
        match client.send_and_delete(filename).await {
            Ok(_) => println!("Successfully sent file"),
            Err(e) => println!("Error sending file: {}", e),
        }
    }
}


#[cfg(test)]
mod tests {
    use security_cam_common::shuttle_runtime::tokio;

    #[tokio::test]
    async fn test_reqwest() {
        reqwest::get("https://httpbin.org/ip").await.unwrap().bytes().await.unwrap();
    }
}