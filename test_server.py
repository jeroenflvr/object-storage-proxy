from object_storage_proxy import start_server, ProxyServerConfig
from dotenv import load_dotenv
import os


load_dotenv()


def docreds(bucket):
    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")
    
    print(f"Fetching credentials for {bucket}")
    return apikey

def do_validation(headers) -> bool:
    return True


def main():

    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")


    cos_mapping = [
        ("bucket1", "s3.eu-de.cloud-object-storage.appdomain.cloud", 443, "instance1", apikey),
        ("bucket2", "s3.eu-de.cloud-object-storage.appdomain.cloud", 443, "instance2", apikey),
        ("proxy-bucket01", "s3.eu-de.cloud-object-storage.appdomain.cloud", 443, "instance3", apikey),
    ]


    ra = ProxyServerConfig(
        bucket_creds_fetcher=docreds,
        validator=do_validation,
        cos_map=cos_mapping,
        port=6190
    )

    start_server(ra)


if __name__ == "__main__":
    main()



