#![warn(clippy::all)]

use parsers::credentials::parse_token_from_header;
use pyo3::prelude::*;

use async_trait::async_trait;
use http::uri::Authority;
use http::Uri;

use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{pyclass, pyfunction, pymodule, wrap_pyfunction, Bound, PyResult, Python};
use std::collections::HashMap;
use std::fmt::Debug;

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
use utils::validator::validate_request;
use credentials::secrets_proxy::{get_bearer, SecretsCache};

static REQ_COUNTER: Mutex<usize> = Mutex::new(0);

#[pyclass]
#[pyo3(name = "ProxyServerConfig")]
#[derive(Debug)]
pub struct ProxyServerConfig {

    #[pyo3(get, set)]
    pub bucket_creds_fetcher: Option<Py<PyAny>>,

    #[pyo3(get, set)]
    pub cos_map: PyObject,

    #[pyo3(get, set)]
    pub port: u16,

    #[pyo3(get, set)]
    pub validator: Option<Py<PyAny>>,

}


impl Default for ProxyServerConfig {
    fn default() -> Self {
        ProxyServerConfig {
            bucket_creds_fetcher: None,
            cos_map: Python::with_gil(|py| py.None()),
            port: 6190,
            validator: None,
        }
    }
}

#[pymethods]
impl ProxyServerConfig {
    #[new]
    pub fn new(bucket_creds_fetcher: Option<PyObject>, cos_map: PyObject, port: u16, validator: Option<Py<PyAny>>) -> Self {
        ProxyServerConfig {
            bucket_creds_fetcher: bucket_creds_fetcher.map(|obj| obj.into()),
            cos_map,
            port,
            validator,
        }
    }
}

#[derive(FromPyObject, Debug, Clone)]
pub struct CosMapItem {
    pub host: String,
    pub port: u16,
    pub instance: String,
    pub api_key: Option<String>,
}


fn parse_cos_map(py: Python, cos_dict: &PyObject) -> PyResult<HashMap<String, CosMapItem>> {
    let mut cos_map: HashMap<String, CosMapItem> = HashMap::new();
    let cos_tuples: Result<Vec<(String, String, u16, String, Option<String>)>, PyErr> = cos_dict.extract(py);

    match cos_tuples {
        Ok(cos_tuples) => {
            for (bucket, host, port, instance, api_key) in cos_tuples {
                let host = host.to_string();
                let instance = instance.to_string();
                let port = port;
                let bucket = bucket.to_string();
                let api_key = api_key.map(|s| s.to_string());

                cos_map.insert(
                    bucket.clone(),
                    CosMapItem {
                        host: host.clone(),
                        port,
                        instance: instance.clone(),
                        api_key: api_key.clone(),
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
    cos_endpoint: String,
    cos_mapping: HashMap<String, CosMapItem>,
    secrets_cache: SecretsCache,
    validator: Option<Py<PyAny>>,
}

pub struct MyCtx {
    cos_mapping: HashMap<String, CosMapItem>,
    secrets_cache: SecretsCache,
    validator: Option<Py<PyAny>>,
}


#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = MyCtx;
    fn new_ctx(&self) -> Self::CTX {
        MyCtx { 
            cos_mapping: self.cos_mapping.clone(),
            secrets_cache: self.secrets_cache.clone(),
            validator: self.validator.as_ref().map(|v| Python::with_gil(|py| v.clone_ref(py))),
         }
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let mut req_counter = REQ_COUNTER.lock().unwrap();
        *req_counter += 1;

        // let host = session.req_header().headers.get("host").unwrap();
        // let host = host.to_str().unwrap();
        let path = session.req_header().uri.path();

        let parse_path_result = parse_path(path);
        if parse_path_result.is_err() {
            eprintln!("Failed to parse path: {:?}", parse_path_result);
            return Err(pingora::Error::new_str("Failed to parse path"));
        }

        let (_, (bucket, _)) = parse_path(path).unwrap();

        let hdr_bucket = bucket.to_owned();

        let bucket_config = ctx.cos_mapping.get(&hdr_bucket);
        let endpoint = match bucket_config {
            Some(config) => {
                format!("{}", config.host)
            }
            None => {
                format!("{}.{}", bucket, self.cos_endpoint)
            }
        };        
        dbg!(&endpoint);

        let addr = (endpoint.clone(), 443);

        let mut peer = Box::new(HttpPeer::new(addr, true, endpoint.clone()));
        peer.options.verify_cert = false;
        Ok(peer)
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut pingora::http::RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        let request_header = session.req_header();
        let auth_header = request_header.headers.get("authorization").unwrap();

        let is_authorized = validate_request(auth_header.to_str().unwrap());

        println!("request is autorized: {:?}", is_authorized);

        let cos_header = parse_token_from_header(auth_header.to_str().unwrap());
        dbg!(&cos_header);

        
        dbg!(&session.req_header());

        let (_, (bucket, my_updated_url)) = parse_path(upstream_request.uri.path()).unwrap();

        let hdr_bucket = bucket.to_string();

        let my_query = match upstream_request.uri.query() {
            Some(q) if !q.is_empty() => format!("?{}", q),
            _ => String::new(),
        };

        let bucket_config = ctx.cos_mapping.get(&hdr_bucket);

        let endpoint = match bucket_config {
            Some(config) => {
                format!("{}.{}", bucket, config.host)
            }
            None => {
                format!("{}.{}", bucket, self.cos_endpoint)
            }
        };
        let api_key = match bucket_config {
            Some(config) => {
                config.api_key.clone()
            }
            None => None,
        };

        let Some(api_key) = api_key else {
            eprintln!("No API key configured for bucket: {}", hdr_bucket);
            return Err(pingora::Error::new_str(
                "No API key configured for bucket",
            ));
        };

        let bearer_fetcher = {
            let api_key = api_key.clone();
            move || get_bearer(api_key.clone())
        };
        
        let bearer_token = ctx.secrets_cache.get(&hdr_bucket, bearer_fetcher).await;

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
            .insert_header("Authorization", format!("Bearer {}", bearer_token.unwrap()))?;
        Ok(())
    }
}

pub fn run_server(py: Python, run_args: &ProxyServerConfig) {
    dbg!(run_args);

    match run_args.bucket_creds_fetcher {
        Some(ref fetcher) => {
            println!("Bucket creds fetcher provided: {:?}", fetcher);
            let _d = get_api_key_for_bucket(py, fetcher, "bucket01".to_string());
        }
        None => {
            println!("No bucket creds fetcher provided");
        }
    }

    let cosmap = parse_cos_map(py, &run_args.cos_map).unwrap();
    dbg!(&cosmap);

    dbg!(&cosmap["bucket1"].instance);

    let mut my_server = Server::new(None).unwrap();
    my_server.bootstrap();



    let mut my_proxy = pingora::proxy::http_proxy_service(
        &my_server.configuration,
        MyProxy {
            cos_endpoint: "s3.eu-de.cloud-object-storage.appdomain.cloud".to_string(),
            cos_mapping: cosmap,
            secrets_cache: SecretsCache::new(),
            validator: None,
        },
    );
    my_proxy.add_tcp("0.0.0.0:6190");

    my_server.add_service(my_proxy);

    my_server.run_forever()
    

}


fn get_api_key_for_bucket(py: Python, callback: &PyObject, bucket: String) -> PyResult<()> {

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