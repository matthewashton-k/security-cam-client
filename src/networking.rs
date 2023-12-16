use std::collections::HashMap;
use std::error::Error;
use reqwest::Url;

pub struct Client<'a> {
    addr: Url,
    username: &'a str,
    password: &'a str,
    client: reqwest::Client,
}

impl<'a> Client<'a> {
    pub fn new(addr: &'a str, username: &'a str, password: &'a str) -> Self {
        // doesnt need to be a recoverable error because if it fails then we want our whole program to exit anyways
        let client_with_cookies = reqwest::Client::builder().cookie_store(true).build().expect("couldn't build http client");
        Client {
            addr: Url::parse(addr).expect("invalid url"),
            username,
            password,
            client:  client_with_cookies,
        }
    }

    pub async fn login(&self) -> Result<(), Box<dyn Error>> {
        let mut params = HashMap::new();
        params.insert("username", self.username);
        params.insert("password", self.password);
        let resp = self.client.post(self.addr).form(&params).send().await?;
        println!("{:?}", resp.text().await);
        Ok(())
    }

    pub async fn logout(&self) -> Result<(), Box<dyn Error>> {
        todo!("");
    }
    pub async fn send_and_delete(&self, filename: String) -> Result<(), Box<dyn Error>> {
        todo!("");
    }
}

#[cfg(test)]
mod tests {
    use security_cam_common::shuttle_runtime::tokio;

    #[tokio::test]
    async fn test_login() {
        let client = super::Client::new("http://127.0.0.1:8000", "admin", "pass");
        client.login().await.unwrap();
        client.logout().await.unwrap();
    }
}