import sys
import os
import json
import requests

if (len(sys.argv) <= 1):
    print("config json file needed")
    sys.exit(1)

if (not sys.argv[1].endswith(".json")):
    print("only json configs supported")
    sys.exit(1)

# Load config
with open(sys.argv[1], "r") as f:
    config = json.load(f)

provider = config["provider"]

url = f'{provider["base_url"].rstrip("/")}/chat/completions'

headers = {
    "Authorization": f'Bearer {provider["api_key"]}',
    "Content-Type": "application/json",
}

payload = {
    "model": provider["model"],
    "messages": [
        {
            "role": "user",
            "content": "Write a Python function that calculates Fibonacci numbers."
        }
    ],
}

response = requests.post(
    url,
    headers=headers,
    json=payload,
    timeout=60,
)

response.raise_for_status()

data = response.json()
print(json.dumps(data, indent=2))

# If OpenAI-compatible:
try:
    print("\nResponse:")
    print(data["choices"][0]["message"]["content"])
except (KeyError, IndexError):
    pass
