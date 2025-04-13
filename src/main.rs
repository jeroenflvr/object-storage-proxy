
use async_trait::async_trait;
use http::uri::Authority;
use http::Uri;

use tokio::runtime::Runtime;
use std::env;
use std::sync::Mutex;

use pingora::server::configuration::Opt;
use pingora::server::Server;
use pingora::upstreams::peer::HttpPeer;
use pingora::Result;
use pingora::proxy::{ProxyHttp, Session};
use dotenv::dotenv;

pub mod parsers;
use parsers::path::parse_path;

pub mod utils;
use utils::creds::get_bearer;

// global counter
static REQ_COUNTER: Mutex<usize> = Mutex::new(0);

pub struct MyProxy {
    bearer_token: String,
    cos_endpoint: String,
}

pub struct MyCtx {
    bearer_token: String,
}


#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = MyCtx;
    fn new_ctx(&self) -> Self::CTX {
        MyCtx { 
            bearer_token: self.bearer_token.clone(),
         }
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let mut req_counter = REQ_COUNTER.lock().unwrap();
        *req_counter += 1;


        let host = session.req_header().headers.get("host").unwrap();
        let host = host.to_str().unwrap();
        println!("------>");
        dbg!(&host);
        let path = session.req_header().uri.path();
        println!("------>");

        dbg!(&path);

        let (_, (bucket, _)) = parse_path(path).unwrap();

        println!("------>");
        dbg!(&bucket);

        let endpoint = format!("{}.{}", bucket, self.cos_endpoint);
        println!("------>");
        dbg!(&endpoint);


        let addr = (endpoint.clone(), 443);

        let mut peer = Box::new(HttpPeer::new(addr, true, endpoint.clone()));
        peer.options.verify_cert = false;
        Ok(peer)
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut pingora::http::RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        dbg!(&upstream_request.headers);
        dbg!(&upstream_request.uri);

        let (_, (bucket, my_updated_url)) = parse_path(upstream_request.uri.path()).unwrap();

        dbg!(&bucket);
        dbg!(&my_updated_url);

        let my_query = match upstream_request.uri.query() {
            Some(q) if !q.is_empty() => format!("?{}", q),
            _ => String::new(),
        };


        let endpoint = format!("{}.{}", bucket, self.cos_endpoint);

        // Box:leak the temporary string to get a static reference which will outlive the function
        let authority = Authority::from_static(Box::leak(endpoint.clone().into_boxed_str()));

        upstream_request.set_uri(
            Uri::builder()
                .authority(
                    upstream_request
                        .uri
                        .authority()
                        .unwrap_or(&authority)
                        .clone(),
                )
                .scheme(upstream_request.uri.scheme_str().unwrap_or("https"))
                .path_and_query(
                    my_updated_url.to_owned() + (&my_query),
                )
                .build()
                .unwrap(),
        );


        upstream_request
            .insert_header("host", endpoint.to_owned())?;
        upstream_request
            .insert_header("Authorization", format!("Bearer {}", _ctx.bearer_token))?;
        dbg!(&upstream_request.headers);
        dbg!(&upstream_request.uri);
        Ok(())
    }
}

fn main() {
    env_logger::init();
    dotenv().ok();
    let api_key = env::var("COS_API_KEY").expect("COS_API_KEY environment variable not set");


    let rt = Runtime::new().expect("Failed to create Tokio runtime");

    let bearer_token = rt.block_on(get_bearer(api_key));
    if let Err(e) = bearer_token {
        eprintln!("Error getting bearer token: {}", e);
        return;
    }

    println!("Bearer token retrieved successfully: {bearer_token:?}");

    let opt = Opt::parse_args();
    let mut my_server = Server::new(Some(opt)).unwrap();
    my_server.bootstrap();

    let mut my_proxy = pingora::proxy::http_proxy_service(
        &my_server.configuration,
        MyProxy {
            bearer_token: bearer_token.unwrap(),
            cos_endpoint: "s3.eu-de.cloud-object-storage.appdomain.cloud".to_string(),
        },
    );
    my_proxy.add_tcp("0.0.0.0:6190");

    my_server.add_service(my_proxy);
    my_server.run_forever();
}

