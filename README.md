# Security Camera Client
A motion detecting, end to end encrypted security camera client program written with reqwest, tokio, and actix-web, and video4linux.
This project is meant to be a client written to interact with the server found at https://github.com/matthewashton-k/security-cam-server.

## Implementation Details
* Motion detection based on double pixel difference calculations
* The program reads in frames and holds on to moving windows of three frames, converting them to greyscale and calculating the
    difference between f1 and f2, and f2 and f3. A threshold is applyed to these two differences,
    so that only a bitmask of the calculated difference is created from each difference. The two differences are bitwise
    Anded together and then the number of pixels in the resulting bitmask is used to detect if movement has occured or not.

* After detecting movement, the program begins by creating a FrameReader with a Stream created by turning the recieving end of a channel
into a Stream with ReceiverStream::new(rx), where the unencrypted frames are sent to the recieving end from the tx side of the channel.
* The FrameReader implements AsyncRead, and is encrypted using the security-cam-common crate I created (https://crates.io/crates/security-cam-common)
* The security-cam-common::encrypt_frame_reader function returns a Stream<Item = Result<Vec<u8>, std::io::Error>> which is turned into a request body by
Body::wrap_stream(stream)
* A new non blocking async task is created which sends the post request to the /upload endpoint of the server and begins streaming the encrypted frames
as they are created.

## Usage
* After running the server found at https://github.com/matthewashton-k/security-cam-server you can then run the client with ```client <username> <passcode> <server address> <video device>``` where video device should be an integer corresponding with whichever video capture device you want to use. (usually 0)
* Movement is detected based on a threshold, and then the program will begin streaming frames to the server until 10 seconds after the movement has stopped.
