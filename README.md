# rust-gcp-logs-analyzer

Focused CLI to work with GCP Cloud Logging from the terminal. It can fetch logs for a time range, tail in near real time, and run insights-style queries. Output is TSV by default or NDJSON with `--json`.

## Steps
- Install Rust (`rustup`)
- Set `GCP_PROJECT` or pass `--project`
- Make sure Application Default Credentials are available (gcloud or a service account)

## Local Usage
```
# Fetch past hour
cargo run -- --project my-proj fetch --filter 'severity>=ERROR' --start -1h

# Tail recent entries
cargo run -- --project my-proj tail --filter 'resource.type="k8s_container"' --json

# Insights-like query
cargo run -- --project my-proj insights --query 'fetch httpRequest.requestMethod, count(*) group by 1' --start -3h --json
```

Time args: RFC3339 (`2025-01-01T00:00:00Z`), milliseconds, or relative (`-15m`, `-2h`).

## Docker
```
docker build -t rust-gcp-logs-analyzer .
```

Run with ADC files mounted:
```
docker run --rm -it ^
  -e GCP_PROJECT=my-proj ^
  -v %UserProfile%\.config\gcloud:/root/.config/gcloud:ro ^
  rust-gcp-logs-analyzer ^
  fetch --filter 'severity>=ERROR' --start -1h --json
```
On Linux/macOS: mount `~/.config/gcloud:/root/.config/gcloud:ro`.

## Docker Compose
```
docker compose run --rm cli --project my-proj fetch --start -30m --json
```

## JSON Examples
```
{"timestamp":"2025-10-16T12:00:00Z","jsonPayload":{"msg":"ok"}}
{"timestamp":"2025-10-16T12:00:05Z","textPayload":"GET / 200"}
```
