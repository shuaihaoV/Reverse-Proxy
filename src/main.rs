use async_trait::async_trait;
use clap::Parser;
use env_logger;
use log::{error, info, warn};
use pingora::prelude::*;
use std::net::SocketAddr;

#[derive(Parser)]
#[command(author, name = "Reverse Proxy", version = "2.0", about = "Reverse Proxy Tools", long_about = None)]
struct ProxyArgs {
    #[arg(
        long,
        short = 'L',
        default_value = "8000",
        help = "Local Listen Port"
    )]
    lport: u16,

    #[arg(long, short = 'I', help = "Remote IP")]
    ip: String,

    #[arg(long, short = 'P', help = "Remote Port", default_value = "6677")]
    port: u16,

    #[arg(long, short = 'H', help = "New Host (Relace Request Header)")]
    host: String,

    #[arg(
        long,
        short = 'X',
        help = "Request Header X-Forwarded-For",
        default_value = "127.0.0.1"
    )]
    x_forwarded_for: String,
}

pub struct MyProxy {
    remote_addr: SocketAddr,
    host: String,
    x_forwarded_for: String,
}

fn main() {
    env_logger::init(); // Initialize the logger

    let args = ProxyArgs::parse();
    let mut my_server = Server::new(None).expect("Failed to create server");
    my_server.bootstrap();

    let remote_addr = match args.ip.parse() {
        Ok(ip) => SocketAddr::new(ip, args.port),
        Err(_) => {
            error!("Invalid IP address: {}", args.ip);
            return;
        }
    };

    let mut proxy_service = http_proxy_service(
        &my_server.configuration,
        MyProxy {
            remote_addr: remote_addr.clone(),
            host: args.host.clone(),
            x_forwarded_for: args.x_forwarded_for.clone(),
        },
    );
    let listen_addr =  &format!("0.0.0.0:{}", args.lport);
    proxy_service.add_tcp(listen_addr);
    my_server.add_service(proxy_service);
    warn!("Server Listen: http://{} Proxy To http://{}:{} Host: {} XFF: {}",listen_addr,args.ip,args.port,args.host,args.x_forwarded_for);
    my_server.run_forever();
}

#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = ();

    fn new_ctx(&self) -> () {
        ()
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let peer = Box::new(HttpPeer::new(&self.remote_addr, false, self.host.clone()));
        Ok(peer)
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        upstream_request.insert_header("Host", &self.host)?;
        upstream_request.insert_header("X-Forwarded-For", &self.x_forwarded_for)?;

        if let Some(referer_url) = upstream_request.headers.get("Referer") {
            let index = str_index(referer_url.to_str().unwrap(), '/', 3);
            if let Ok(new_referer_url) = format!(
                "https://{}{}",
                &self.host,
                if index!=0 {&referer_url.to_str().unwrap()[index - 1..]} else {""}
            )
            .parse::<String>()
            {
                if let Err(err) = upstream_request.insert_header("Referer", new_referer_url) {
                    upstream_request.remove_header("Referer");
                    warn!("insert Referer Error {}", err.to_string());
                }
            }
        }

        if let Some(origin_url) = upstream_request.headers.get("Origin") {
            let index = str_index(origin_url.to_str().unwrap(), '/', 3);

            if let Ok(new_origin_url) = format!(
                "https://{}{}",
                &self.host,
                if index!=0 {&origin_url.to_str().unwrap()[index - 1..]} else {""}
            )
            .parse::<String>()
            {
                if let Err(err) = upstream_request.insert_header("Origin", new_origin_url) {
                    upstream_request.remove_header("Origin");
                    warn!("insert Origin Error {}", err.to_string());
                }
            }
        }

        Ok(())
    }

    async fn logging(
        &self,
        session: &mut Session,
        _e: Option<&pingora::Error>,
        _ctx: &mut Self::CTX,
    ) {
        let response_code = session
            .response_written()
            .map_or(0, |resp| resp.status.as_u16());
        
        info!(
            "{} {} {}",
            session.req_header().method,
            response_code,
            session.req_header().uri,
        );
    }
}

fn str_index(s: &str, tc: char, num: i32) -> usize {
    let mut count = 0;
    let mut start_index = 0;
    for (index, c) in s.chars().enumerate() {
        if c == tc {
            count += 1;
            if count == num {
                start_index = index + 1;
                break;
            }
        }
    }
    return start_index;
}
