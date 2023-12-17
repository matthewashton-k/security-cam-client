# Security Camera Client

This project is meant to be a client written to interact with my secure mp4 host project (https://gitlab.com/matthewashton_k/secure-mp4-host)


## Features/Implementation Details

* Motion detection based on double pixel difference calculations
    * The program reads in frames and holds on to moving windows of three frames, converting them to greyscale and calculating the difference between f1 and f2, and f2 and f3. A threshold is applyed to these two differences, so that only a bitmask of the calculated difference is created from each difference. The two differences are bitwise Anded together and then the number of pixels in the resulting bitmask is used to detect if movement has occured or not.

* After detecting movement, the program begins saving frames to video_frames/ with the format <video num>.<frame num>.jpg 
* If ten seconds elapse without any new movement, a command containing the video num and the largest frame num corresponding with that video num is send from the frame processing thread to the main thread.
* in the main thread all of the frames corresponding to <video num> are compiled together into an mp4 using ffmpeg, then the frames are removed from the file system.
* The resulting mp4 file is encrypted and sent in a stream to the server where it is saved.
* encryption is handeled by common crate I created (https://crates.io/crates/security-cam-common)


## Usage
* follow installation instructions for the server at https://gitlab.com/matthewashton_k/secure-mp4-host
* after running the server you can then run the client with ```client <username> <passcode> <address> <video device>``` where video device should be an integer corresponding with whichever video capture device you want to use. (usually 0)
* once you start moving around (then waiting with no movement for 10 seconds) you should see the output of ffmpeg and then a status code signifying the mp4 has been sent to the server
