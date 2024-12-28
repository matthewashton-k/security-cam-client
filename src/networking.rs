use actix_web::rt::task::JoinHandle;
use actix_web::web::Bytes;
use awc::error::{WsClientError, WsProtocolError};
use awc::ws::Message;
use awc::ClientResponse;
use reqwest::redirect::Policy;
use reqwest::{Body, Url};
use reqwest_websocket::{RequestBuilderExt, UpgradedRequestBuilder, WebSocket};
use security_cam_common::encryption::FrameReader;
use security_cam_common::encryption::*;
use security_cam_common::futures::{Sink, SinkExt, StreamExt, TryStreamExt};
use security_cam_common::shuttle_runtime::tokio::fs::File;
use security_cam_common::shuttle_runtime::tokio::sync::mpsc::{channel, Receiver, Sender};
use security_cam_common::shuttle_runtime::tokio::sync::{Barrier, Notify};
use security_cam_common::shuttle_runtime::tokio::{self, fs};
use security_cam_common::tokio_stream::wrappers::ReceiverStream;
use std::collections::HashMap;
use std::error::Error;
use std::io::ErrorKind::{self, NotFound};
use std::io::{Cursor, Read};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::motiondetection::Frame;
pub struct Client<'a> {
    addr: Url,
    username: &'a str,
    password: &'a str,
    client: reqwest::Client,
    pub tx: Option<Sender<Result<Bytes, std::io::Error>>>,
    pub frame_stream: Option<Pin<Box<ReceiverStream<Result<Bytes, std::io::Error>>>>>,
    pub transfer_task: Option<JoinHandle<Result<(), Box<dyn std::error::Error>>>>,
    // client_awc: awc::Client,
    // websocket_connection_awc: Option<Box<dyn Sink<Message, Error = WsProtocolError> + Unpin>>,
    // websocket_connection: Option<reqwest_websocket::WebSocket>,
}

impl<'a> Client<'a> {
    // pub async fn close_ws(&mut self) -> Result<(), Box<dyn Error>> {
    //     if let Some(ws) = self.websocket_connection.take() {
    //         ws.close(reqwest_websocket::CloseCode::Normal, None).await?;
    //     }
    //     if let Some(mut ws) = self.websocket_connection_awc.take() {
    //         ws.close().await?;
    //     }
    //     self.websocket_connection = None;
    //     self.websocket_connection_awc = None;
    //     Ok(())
    // }

    pub async fn new(addr: &'a str, username: &'a str, password: &'a str) -> Client<'a> {
        // doesnt need to be a recoverable error because if it fails then we want our whole program to exit anyways
        let client_with_cookies = reqwest::Client::builder()
            .redirect(Policy::limited(2))
            .cookie_store(true)
            .tcp_keepalive(Duration::from_secs(300))
            .build()
            .expect("couldn't build http client");
        Client {
            addr: Url::parse(addr).expect("invalid url"),
            username,
            password,
            client: client_with_cookies,
            tx: None,
            frame_stream: None,
            transfer_task: None,
            // client_awc,
            // websocket_connection_awc: None,
            // websocket_connection: None,
        }
    }

    pub async fn login(&self) -> Result<(), Box<dyn Error>> {
        let mut params = HashMap::new();
        println!("logging in with {} {}", self.username, self.password);
        params.insert("username", self.username);
        params.insert("password", self.password);
        let resp = self
            .client
            .post(self.addr.join("login")?.as_str())
            .form(&params)
            .send()
            .await?;

        // later on change the webserver to alter the response code if the login fails
        let text = resp.text().await?;
        if !text.contains("Logout") {
            println!("{}", text);
            return Err("login failed".into());
        }

        Ok(())
    }

    pub async fn logout(&self) -> Result<(), Box<dyn Error>> {
        let resp = self
            .client
            .get(self.addr.join("logout")?.as_str())
            .send()
            .await?;

        // later on change the webserver to alter the response code if the logout fails
        if !resp.text().await?.contains("Login") {
            return Err("logout failed".into());
        }
        Ok(())
    }

    #[deprecated]
    pub async fn send_and_delete(&self, filename: String) -> Result<(), Box<dyn Error>> {
        let (key, salt) = generate_key(self.password).expect("couldnt generate keystream");

        let file = File::options()
            .read(true)
            .write(true)
            .open(&filename)
            .await?;
        let stream = Box::pin(encrypt_stream(key, salt, file));
        let resp = self
            .client
            .post(self.addr.join("new_video")?.as_str())
            .body(Body::wrap_stream(stream))
            .send()
            .await?;
        println!("status: {:?}, text: {:?}", resp.status(), resp.text().await,);
        fs::remove_file(&filename).await?;
        Ok(())
    }

    /// sends a range of frames that have all been saved to files locally
    #[deprecated]
    pub async fn send_frame_range(
        &self,
        video_num: usize,
        frame_count: usize,
        fps: usize,
    ) -> Result<(), Box<dyn Error>> {
        let (key, salt) = generate_key(self.password).expect("couldnt generate keystream");
        //Option<Box<dyn Stream<Item=Result<Vec<u8>, std::io::Error>>>>
        let mut enc_stream: Option<
            Pin<
                Box<
                    dyn security_cam_common::tokio_stream::Stream<
                            Item = Result<Vec<u8>, std::io::Error>,
                        > + Send
                        + Sync,
                >,
            >,
        > = None;
        for i in 0..frame_count {
            let filename = format!("video_frames/{}.{}.jpg", video_num, i);
            let file = File::options()
                .read(true)
                .write(true)
                .open(&filename)
                .await?;
            let mut stream: Pin<
                Box<
                    dyn security_cam_common::tokio_stream::Stream<
                            Item = Result<Vec<u8>, std::io::Error>,
                        > + Send
                        + Sync,
                >,
            > = Box::pin(encrypt_stream(key, salt.clone(), file));
            if enc_stream.is_none() {
                enc_stream = Some(stream);
            } else {
                enc_stream = Some(Box::pin(enc_stream.unwrap().chain(stream)));
            }
            fs::remove_file(&filename).await?;
        }

        let url = self
            .addr
            .join("new_video_ffmpeg/")?
            .join(video_num.to_string().to_path())?
            .join(&"e".to_string().to_path())?
            .join(&frame_count.to_string().to_path())?
            .join(&fps.to_string().to_path())?;
        let resp = self
            .client
            .post(url.as_str())
            .body(Body::wrap_stream(
                enc_stream.ok_or(std::io::Error::new(NotFound, "no file to encrypt"))?,
            ))
            .send()
            .await?;

        Ok(())
    }

    pub async fn send_frame_framereader(&mut self, frame: Frame) -> Result<(), Box<dyn Error>> {
        // sending frames to main thread ->
        // main thread calls send_frame_framereader
        // this thread checks if there is an ongoing recording
        // if not make a new tx rx channel
        //      make framereader from rx channel
        //      store framereader in self
        //      open connection to server
        //      spawn a new task that sends the output of encrypt_frame_reader to server
        // if not, then send frame on tx channel
        if self.tx.is_none() {
            let (tx, rx) = channel(5);
            self.tx = Some(tx.clone());
            let frame_len = frame.frame_bytes.len();
            tx.send(Ok(Bytes::from(frame.frame_bytes)))
                .await
                .expect("failed to send the frame butes to the receiver stream");

            // start the transfer task
            let client = self.client.clone();
            let password = self.password.to_string();
            let addr = self.addr.clone();
            let transfer_task = actix_web::rt::spawn(async move {
                let framereader = FrameReader::new(ReceiverStream::new(rx));
                let (key, salt) = generate_key(&password).expect("couldnt generate keystream");
                let encrypted_frame_stream = {
                    let stream = encrypt_frame_reader(key, salt, framereader, frame_len);
                    Box::pin(stream)
                };
                let url = addr
                    .join("upload/")?
                    .join(&frame.video_num.to_string().to_path())?
                    .join(&frame.fps.to_string().to_path())?
                    .join(frame_len.to_string().as_ref())?;
                if let Err(e) = async {
                    client
                        .post(url)
                        .body(Body::wrap_stream(encrypted_frame_stream))
                        .send()
                        .await?;
                    Ok::<(), Box<dyn std::error::Error>>(())
                }
                .await
                {
                    eprintln!("[ERROR] Spawned task failed: {}", e);
                    return Err(e);
                }
                Ok::<(), Box<dyn std::error::Error>>(())
            });
            self.transfer_task = Some(transfer_task);
        } else {
            let result = self
                .tx
                .clone()
                .unwrap()
                .send(Ok(Bytes::from(frame.frame_bytes)))
                .await;
            if result.is_err() {
                eprintln!(
                    "[ERROR] sending over tx: {}",
                    result.as_ref().err().unwrap().to_string()
                );
                return result.map_err(|e| e.into());
            }
        }

        Ok(())
    }

    // #[deprecated]
    // pub async fn send_frame_ws(&mut self, frame: Frame) -> Result<(), Box<dyn Error>> {
    //     // need new salt each time
    //     let (key, salt) = generate_key(self.password).expect("couldnt generate keystream");
    //     let mut stream = Box::pin(encrypt_stream_frame(
    //         key,
    //         salt,
    //         Cursor::new(frame.frame_bytes),
    //     ));

    //     // create the decrypted frame
    //     let mut encrypted_frame: Vec<u8> = Vec::new();
    //     while let Some(Ok(chunk)) = stream.next().await {
    //         encrypted_frame.extend(chunk);
    //     }

    //     println!("got here, abt to check websocket connection private member");
    //     if self.websocket_connection.is_none() {
    //         let url = self
    //             .addr
    //             .join("new_video_stream/")?
    //             .join(&frame.video_num.to_string().to_path())?
    //             .join(&frame.fps.to_string())?;
    //         let connection = self
    //             .client
    //             .get(url)
    //             .upgrade()
    //             .send()
    //             .await?
    //             .into_websocket()
    //             .await?;
    //         self.websocket_connection = Some(connection);
    //     }
    //     println!("got here");
    //     if let Some(ws) = &mut self.websocket_connection {
    //         println!("sending frame over websocket connection");
    //         let (mut tx, mut rx) = ws.split();
    //         security_cam_common::futures::future::join(
    //             async move {
    //                 tx.send(reqwest_websocket::Message::Binary(encrypted_frame))
    //                     .await
    //                     .expect("shit");
    //             },
    //             async move {
    //                 while let Some(message) = rx.try_next().await.unwrap() {
    //                     if let reqwest_websocket::Message::Text(text) = message {
    //                         println!("received: {text}");
    //                     }
    //                 }
    //             },
    //         )
    //         .await;
    //     }
    //     Ok(())
    // }
}

trait ToPathSegment {
    fn to_path(&mut self) -> &str;
}

impl ToPathSegment for String {
    fn to_path(&mut self) -> &str {
        self.push('/');
        return self;
    }
}

#[cfg(test)]
mod tests {
    use security_cam_common::shuttle_runtime::tokio;

    /// only run this test while the server is active
    #[tokio::test]
    async fn test_login() {
        let client = super::Client::new("http://127.0.0.1:8000", "admin", "pass").await;
        client.login().await.unwrap();
    }

    #[tokio::test]
    async fn test_bad_login() {
        let client = super::Client::new("http://127.0.0.1:8000", "admin", "badpass").await;
        client.login().await.unwrap_err();
    }

    #[tokio::test]
    async fn test_logout() {
        let client = super::Client::new("http://127.0.0.1:8000", "admin", "pass").await;
        client.login().await.unwrap();
        client.logout().await.unwrap();
    }

    #[tokio::test]
    async fn test_send_video() {
        let client = super::Client::new("http://127.0.0.1:8000", "admin", "pass").await;
        client.login().await.unwrap();
        client
            .send_and_delete("test.mp4".to_string())
            .await
            .unwrap();
    }
}
