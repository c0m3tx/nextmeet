use oauth2::basic::BasicClient;
use oauth2::reqwest::http_client;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl,
};
use reqwest::Url;
use serde::Deserialize;
use serde::Serialize;
use std::error::Error;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::net::TcpListener;
use std::process::Command;

#[derive(Serialize, Deserialize, Debug)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

fn config_path() -> String {
    std::env::var_os("HOME")
        .map(|var| var.to_str().unwrap().to_owned())
        .unwrap()
        + "/.nextmeet"
}

impl Tokens {
    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        std::fs::write(config_path(), serde_json::to_string(&self)?)
            .map_err(|_| "Error saving tokens to disk".into())
    }

    pub fn load() -> Result<Tokens, Box<dyn Error>> {
        let token = std::fs::read_to_string(config_path()).map_err(|_| "File not found")?;
        serde_json::from_str::<Tokens>(&token).map_err(|_| "Failed to parse file".into())
    }

    pub fn refresh(self) -> Result<Tokens, Box<dyn Error>> {
        let client_id = crate::config::CLIENT_ID;
        let client_secret = crate::config::CLIENT_SECRET;

        if let Some(refresh_token_str) = self.refresh_token {
            let client = BasicClient::new(
                ClientId::new(client_id.to_string()),
                Some(ClientSecret::new(client_secret.to_string())),
                AuthUrl::new("https://accounts.google.com/o/oauth2/auth".to_string())?,
                Some(TokenUrl::new(
                    "https://oauth2.googleapis.com/token".to_string(),
                )?),
            );
            let refresh_token = RefreshToken::new(refresh_token_str.clone());
            let tokens = client
                .exchange_refresh_token(&refresh_token)
                .request(http_client)
                .map(|res| Tokens {
                    access_token: res.access_token().secret().to_string(),
                    refresh_token: res
                        .refresh_token()
                        .map(|token| token.secret().to_string())
                        .or_else(|| Some(refresh_token_str)),
                })
                .map_err(|_| "Failed to refresh tokens")?;

            tokens.save()?;

            Ok(tokens)
        } else {
            Err("No refresh token available".into())
        }
    }

    pub fn do_login() -> Result<Tokens, Box<dyn Error>> {
        let client_id = crate::config::CLIENT_ID;
        let client_secret = crate::config::CLIENT_SECRET;

        let client = BasicClient::new(
            ClientId::new(client_id.to_string()),
            Some(ClientSecret::new(client_secret.to_string())),
            AuthUrl::new("https://accounts.google.com/o/oauth2/auth".to_string())?,
            Some(TokenUrl::new(
                "https://oauth2.googleapis.com/token".to_string(),
            )?),
        )
        .set_redirect_url(RedirectUrl::new("http://127.0.0.1:35426/auth".to_string())?.into());

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        // Generate the full authorization URL.
        let (auth_url, _) = client
            .authorize_url(CsrfToken::new_random)
            // Set the desired scopes.
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar.events.readonly".to_string(),
            ))
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar.readonly".to_string(),
            ))
            // Set the PKCE code challenge.
            .set_pkce_challenge(pkce_challenge)
            .url();

        // This is the URL you should redirect the user to, in order to trigger the authorization
        // process.

        match Command::new("xdg-open").arg(auth_url.to_string()).output() {
            Ok(_) => {}
            Err(_) => eprintln!("Failed to open browser automatically. Go to {}", auth_url),
        }

        let mut code: Option<String> = None;
        let listener = TcpListener::bind("127.0.0.1:35426").unwrap();
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                {
                    let mut reader = BufReader::new(&stream);

                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();

                    let redirect_url = request_line.split_whitespace().nth(1).unwrap();
                    let url = Url::parse(&("http://localhost".to_string() + redirect_url)).unwrap();

                    code = url
                        .query_pairs()
                        .find(|pair| {
                            let &(ref key, _) = pair;
                            key == "code"
                        })
                        .map(|(_, value)| value.to_string());
                }

                let message = "Go back to your terminal :)";
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                    message.len(),
                    message
                );
                stream.write_all(response.as_bytes()).unwrap();

                break;
            }
        }

        let code = code.expect("No code received");

        let tokens = client
            .exchange_code(AuthorizationCode::new(code))
            // Set the PKCE code verifier.
            .set_pkce_verifier(pkce_verifier)
            .request(http_client)
            .map(|res| Tokens {
                access_token: res.access_token().secret().to_string(),
                refresh_token: res.refresh_token().map(|token| token.secret().to_string()),
            })
            .map_err(|_| "Failed to get access token")?;

        tokens.save()?;
        Ok(tokens)
    }
}
