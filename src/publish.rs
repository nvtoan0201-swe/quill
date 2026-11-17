//! File › Publish: send the current document as a draft to WordPress, Ghost,
//! Micro.blog (Micropub) or Medium. Requests are built by pure functions so they
//! can be tested; `publish` performs them.

use std::path::PathBuf;

use anyhow::{Context as _, Result, anyhow, bail};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Service {
    WordPress,
    Ghost,
    MicroBlog,
    Medium,
}

impl Service {
    pub const ALL: [Service; 4] = [Service::WordPress, Service::Ghost, Service::MicroBlog, Service::Medium];

    pub fn label(self) -> &'static str {
        match self {
            Service::WordPress => "WordPress",
            Service::Ghost => "Ghost",
            Service::MicroBlog => "Micro.blog",
            Service::Medium => "Medium",
        }
    }

    /// Hints for the account form: (site URL, user name, token).
    pub fn fields(self) -> [&'static str; 3] {
        match self {
            Service::WordPress => ["https://example.com", "User name", "Application password"],
            Service::Ghost => ["https://example.ghost.io", "(not used)", "Admin API key (id:secret)"],
            Service::MicroBlog => ["https://micro.blog/micropub", "(not used)", "App token"],
            Service::Medium => ["https://api.medium.com", "(not used)", "Integration token"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub service: Service,
    pub url: String,
    #[serde(default)]
    pub user: String,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub title: String,
    pub html: String,
    pub tags: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

fn base(url: &str) -> &str {
    url.trim_end_matches('/')
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Ghost Admin API token: HS256 JWT signed with the hex secret of "id:secret".
pub fn ghost_jwt(key: &str, now: u64) -> Result<String> {
    let (id, secret) = key.split_once(':').context("Ghost key must look like id:secret")?;
    let secret = (0..secret.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(secret.get(i..i + 2).unwrap_or("x"), 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| anyhow!("Ghost secret must be hex"))?;
    let header = serde_json::json!({ "alg": "HS256", "typ": "JWT", "kid": id });
    let payload = serde_json::json!({ "iat": now, "exp": now + 300, "aud": "/admin/" });
    let signing = format!("{}.{}", b64url(header.to_string().as_bytes()), b64url(payload.to_string().as_bytes()));
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret)?;
    mac.update(signing.as_bytes());
    Ok(format!("{signing}.{}", b64url(&mac.finalize().into_bytes())))
}

fn form(pairs: &[(&str, &str)]) -> String {
    let enc = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                b' ' => "+".to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

/// The request that creates a draft. Medium needs the user id first (`medium_user`).
pub fn draft_request(a: &Account, p: &Post, now: u64, medium_user: Option<&str>) -> Result<Request> {
    let json = ("Content-Type".to_string(), "application/json".to_string());
    Ok(match a.service {
        Service::WordPress => {
            let auth = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", a.user, a.token));
            Request {
                method: "POST",
                url: format!("{}/wp-json/wp/v2/posts", base(&a.url)),
                headers: vec![json, ("Authorization".into(), format!("Basic {auth}"))],
                body: serde_json::json!({ "title": p.title, "content": p.html, "status": "draft" }).to_string(),
            }
        }
        Service::Ghost => Request {
            method: "POST",
            url: format!("{}/ghost/api/admin/posts/?source=html", base(&a.url)),
            headers: vec![json, ("Authorization".into(), format!("Ghost {}", ghost_jwt(&a.token, now)?))],
            body: serde_json::json!({ "posts": [{
                "title": p.title, "html": p.html, "status": "draft",
                "tags": p.tags.iter().map(|t| serde_json::json!({ "name": t })).collect::<Vec<_>>()
            }] })
            .to_string(),
        },
        Service::MicroBlog => {
            let mut pairs = vec![("h", "entry"), ("name", p.title.as_str()), ("content", p.html.as_str()), ("post-status", "draft")];
            for t in &p.tags {
                pairs.push(("category[]", t.as_str()));
            }
            Request {
                method: "POST",
                url: base(&a.url).to_string(),
                headers: vec![
                    ("Content-Type".into(), "application/x-www-form-urlencoded".into()),
                    ("Authorization".into(), format!("Bearer {}", a.token)),
                ],
                body: form(&pairs),
            }
        }
        Service::Medium => Request {
            method: "POST",
            url: format!("{}/v1/users/{}/posts", base(&a.url), medium_user.context("Medium user id missing")?),
            headers: vec![json, ("Authorization".into(), format!("Bearer {}", a.token))],
            body: serde_json::json!({
                "title": p.title, "contentFormat": "html", "content": p.html,
                "publishStatus": "draft", "tags": p.tags.iter().take(5).collect::<Vec<_>>()
            })
            .to_string(),
        },
    })
}

fn send(r: &Request) -> Result<ureq::Response> {
    let mut req = ureq::request(r.method, &r.url);
    for (k, v) in &r.headers {
        req = req.set(k, v);
    }
    match req.send_string(&r.body) {
        Ok(resp) => Ok(resp),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            bail!("server answered {code}: {}", text.chars().take(200).collect::<String>())
        }
        Err(e) => Err(e.into()),
    }
}

/// Create a draft; returns the URL of the new post when the service reports one.
pub fn publish(a: &Account, p: &Post) -> Result<String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let medium_user = if a.service == Service::Medium {
        let me = send(&Request {
            method: "GET",
            url: format!("{}/v1/me", base(&a.url)),
            headers: vec![("Authorization".into(), format!("Bearer {}", a.token))],
            body: String::new(),
        })?;
        let v: serde_json::Value = me.into_json()?;
        Some(v["data"]["id"].as_str().context("Medium did not return a user id")?.to_string())
    } else {
        None
    };
    let resp = send(&draft_request(a, p, now, medium_user.as_deref())?)?;
    if a.service == Service::MicroBlog {
        return Ok(resp.header("Location").unwrap_or_default().to_string());
    }
    let v: serde_json::Value = resp.into_json().unwrap_or_default();
    let url = match a.service {
        Service::WordPress => v["link"].as_str(),
        Service::Ghost => v["posts"][0]["url"].as_str(),
        Service::Medium => v["data"]["url"].as_str(),
        Service::MicroBlog => None,
    };
    Ok(url.unwrap_or_default().to_string())
}

/// Title from the first heading, body without it, tags from #hashtags.
pub fn post_from_markdown(md: &str) -> Post {
    let title = crate::markdown::title(md);
    let mut lines = md.lines();
    let first_heading = md.lines().find(|l| !l.trim().is_empty()).is_some_and(|l| l.trim_start().starts_with("# "));
    let body: String = if first_heading {
        let mut skipped = false;
        lines
            .by_ref()
            .filter(|l| {
                if !skipped && l.trim_start().starts_with("# ") {
                    skipped = true;
                    return false;
                }
                true
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        md.to_string()
    };
    let flags = crate::markdown::style(md);
    let mut tags = Vec::new();
    let mut i = 0;
    while i < flags.len() {
        if flags[i] & crate::markdown::HASHTAG != 0 {
            let s = i;
            while i < flags.len() && flags[i] & crate::markdown::HASHTAG != 0 {
                i += 1;
            }
            let t = md[s + 1..i].to_string();
            if !tags.contains(&t) {
                tags.push(t);
            }
        } else {
            i += 1;
        }
    }
    Post { title, html: crate::export::body_html(&body), tags }
}

// ---- account storage (0600 file in the config directory) -------------------------

fn accounts_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("quill").join("accounts.json"))
}

pub fn load_accounts() -> Vec<Account> {
    accounts_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_accounts(accounts: &[Account]) -> Result<()> {
    let p = accounts_path().context("no config directory")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(accounts)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    fn post() -> Post {
        Post { title: "Hello".into(), html: "<p>Body</p>".into(), tags: vec!["rust".into()] }
    }

    fn account(service: Service, url: &str, token: &str) -> Account {
        Account { service, url: url.into(), user: "me".into(), token: token.into() }
    }

    #[test]
    fn requests_have_the_right_shape() {
        let wp = draft_request(&account(Service::WordPress, "https://x.com/", "pw"), &post(), 0, None).unwrap();
        assert_eq!(wp.url, "https://x.com/wp-json/wp/v2/posts");
        assert!(wp.headers.contains(&("Authorization".into(), "Basic bWU6cHc=".into())));
        assert!(wp.body.contains("\"status\":\"draft\""));

        let mb = draft_request(&account(Service::MicroBlog, "https://micro.blog/micropub", "t"), &post(), 0, None).unwrap();
        assert_eq!(mb.body, "h=entry&name=Hello&content=%3Cp%3EBody%3C%2Fp%3E&post-status=draft&category%5B%5D=rust");

        let md = draft_request(&account(Service::Medium, "https://api.medium.com", "t"), &post(), 0, Some("u1")).unwrap();
        assert_eq!(md.url, "https://api.medium.com/v1/users/u1/posts");
        assert!(md.body.contains("\"publishStatus\":\"draft\""));
    }

    #[test]
    fn ghost_token_is_a_valid_hs256_jwt() {
        let jwt = ghost_jwt("abc:00ff", 1000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[0]).unwrap();
        assert!(String::from_utf8(header).unwrap().contains("\"kid\":\"abc\""));
        // Recompute the signature independently.
        let mut mac = Hmac::<Sha256>::new_from_slice(&[0x00, 0xff]).unwrap();
        mac.update(format!("{}.{}", parts[0], parts[1]).as_bytes());
        assert_eq!(b64url(&mac.finalize().into_bytes()), parts[2]);
        assert!(ghost_jwt("nocolon", 0).is_err());
    }

    #[test]
    fn markdown_becomes_post() {
        let p = post_from_markdown("# My Title\n\nText with #idea and #idea.\n");
        assert_eq!(p.title, "My Title");
        assert!(!p.html.contains("My Title"));
        assert!(p.html.contains("Text with"));
        assert_eq!(p.tags, vec!["idea".to_string()]);
    }

    #[test]
    fn wordpress_round_trip_against_local_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let reply = r#"{"id":7,"link":"http://site/?p=7"}"#;
            write!(stream, "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{reply}", reply.len()).unwrap();
            (head, String::from_utf8(body).unwrap())
        });
        let url = publish(&account(Service::WordPress, &format!("http://{addr}"), "pw"), &post()).unwrap();
        let (head, body) = server.join().unwrap();
        assert_eq!(url, "http://site/?p=7");
        assert!(head.starts_with("POST /wp-json/wp/v2/posts"));
        assert!(head.contains("Authorization: Basic bWU6cHc="));
        assert!(body.contains("\"title\":\"Hello\""));
    }
}
