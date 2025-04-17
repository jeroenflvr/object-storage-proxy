from object_storage_proxy import start_server, ProxyServerConfig


def docreds(bucket):
    print(f"Fetching credentials for {bucket}")
    return f"{bucket}-creds"


cos_mapping = [
    ("bucket1", "us-de", 2222, "instance1", "api_key1"),
    ("bucket2", "us-east-1", 3333, "instance2", "api_key2"),
]


ra = ProxyServerConfig(
    endpoint="endpoint", bucket_creds_fetcher=docreds, cos_map=cos_mapping
)

start_server(ra)
