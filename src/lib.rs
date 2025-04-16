use pyo3::prelude::*;

use async_trait::async_trait;
use http::uri::Authority;
use http::Uri;

use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{pyclass, pyfunction, pymodule, wrap_pyfunction, Bound, PyResult, Python};
use std::collections::HashMap;
use std::env;

use std::sync::Mutex;


use pingora::server::Server;
use pingora::upstreams::peer::HttpPeer;
use pingora::Result;
use pingora::proxy::{ProxyHttp, Session};
use dotenv::dotenv;

pub mod parsers;
use parsers::path::parse_path;

pub mod credentials;

pub mod utils;
use utils::creds::get_bearer;

static REQ_COUNTER: Mutex<usize> = Mutex::new(0);

#[pyclass]
#[pyo3(name = "ProxyServerConfig")]
#[derive(Debug)]
pub struct ProxyServerConfig {
    #[pyo3(get, set)]
    pub endpoint: String,

    #[pyo3(get, set)]
    pub bucket_creds_fetcher: Py<PyAny>,

    #[pyo3(get, set)]
    pub cos_map: PyObject,
}

#[pymethods]
impl ProxyServerConfig {
    #[new]
    pub fn new(endpoint: String, bucket_creds_fetcher: PyObject, cos_map: PyObject) -> Self {
        ProxyServerConfig {
            endpoint,
            bucket_creds_fetcher,
            cos_map,
        }
    }
}

#[derive(FromPyObject, Debug)]
pub struct CosMapItem {
    pub region: String,
    pub port: u16,
    pub instance: String,
}


fn parse_cos_map(py: Python, cos_dict: &PyObject) -> PyResult<HashMap<String, CosMapItem>> {
    let mut cos_map: HashMap<String, CosMapItem> = HashMap::new();
    let cos_tuples: Result<Vec<(String, String, u16, String)>, PyErr> = cos_dict.extract(py);

    match cos_tuples {
        Ok(cos_tuples) => {
            for (bucket, region, port, instance) in cos_tuples {
                let region = region.to_string();
                let instance = instance.to_string();
                let port = port;
                let bucket = bucket.to_string();

                cos_map.insert(
                    bucket.clone(),
                    CosMapItem {
                        region: region.clone(),
                        port,
                        instance: instance.clone(),
                    },
                );
                // println!("Bucket: {}, Region: {}, Port: {}, Instance: {}", bucket, region, port, instance);
            }

            Ok(cos_map)

        }
        Err(e) => {
            eprintln!("Error extracting cos_map: {:?}", e);
            Err(e)
        }
    }

}


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
        dbg!(&host);
        let path = session.req_header().uri.path();

        dbg!(&path);

        let (_, (bucket, _)) = parse_path(path).unwrap();

        dbg!(&bucket);

        let endpoint = format!("{}.{}", bucket, self.cos_endpoint);
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

pub fn run_server(py: Python, run_args: &ProxyServerConfig) {
    dbg!(run_args);

    let _d = get_creds_for_bucket(py, &run_args.bucket_creds_fetcher, "bucket01".to_string());

    let cosmap = parse_cos_map(py, &run_args.cos_map).unwrap();
    dbg!(&cosmap);

    dbg!(&cosmap["bucket1"].instance);

    dotenv().ok();
    env_logger::init();

    let api_key = env::var("COS_API_KEY").expect("COS_API_KEY environment variable not set");

    let bearer_token = get_bearer(api_key);
    if let Err(e) = bearer_token {
        eprintln!("Error getting bearer token: {}", e);
        return;
    }

    // println!("Bearer token retrieved successfully: {bearer_token:?}");

    let mut my_server = Server::new(None).unwrap();
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

    my_server.run_forever()
    

}


fn get_creds_for_bucket(py: Python, callback: &PyObject, bucket: String) -> PyResult<()> {
    
    match callback.call1(py, (bucket,)) {
        Ok(result) => {
            let content = result.extract::<String>(py)?;
            println!("Callback returned: {:?}", content);
            Ok(())
        }
        Err(err) => {
            eprintln!("Python callback raised an exception: {:?}", err);

            // Option 1: Return the error to Python (so Python sees the exception)
            // return Err(err);

            // Option 2: Convert it into a custom Python exception or a new error message
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Failed to call callback due to an inner Python exception",
            ));
        }
    }
}

#[pyfunction]
pub fn jahallo(_py: Python) -> PyResult<String>{
    Ok("jahallo".to_string())
}

#[pyfunction]
pub fn start_server(py: Python, run_args: &ProxyServerConfig) -> PyResult<()> {
    dotenv().ok();

    run_server(py, &run_args);

    Ok(())
}


#[pymodule]
fn object_storage_proxy(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(jahallo, m)?)?;
    m.add_function(wrap_pyfunction!(start_server, m)?)?;
    m.add_class::<ProxyServerConfig>()?;
    Ok(())
}