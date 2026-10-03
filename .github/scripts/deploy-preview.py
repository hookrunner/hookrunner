import json, os, time, urllib.parse, urllib.request

base = os.environ['PREVIEW_API_URL'].rstrip('/')
token = os.environ['PREVIEW_TOKEN']
if not token:
    raise SystemExit('Set the PREVIEW_TOKEN repository secret.')
headers = {'Authorization': f'Bearer {token}'}
query = {'sha': os.environ['DEPLOY_SHA']}
if os.environ.get('PR_NUMBER'):
    query['pr'] = os.environ['PR_NUMBER']
else:
    query['branch'] = os.environ['DEPLOY_BRANCH']
with open('dist/preview.tar.gz', 'rb') as f:
    bundle = f.read()
request = urllib.request.Request(
    f"{base}/deployments?{urllib.parse.urlencode(query)}",
    data=bundle, headers={**headers, 'Content-Type': 'application/gzip'}, method='POST')
with urllib.request.urlopen(request, timeout=300) as response:
    job = json.load(response)
for _ in range(120):
    request = urllib.request.Request(f"{base}/deployments/{job['id']}", headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        job = json.load(response)
    if job['status'] == 'ready':
        print(f"Deployed {query} at {job['url']}")
        with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
            f.write(f"url={job['url']}\n")
        break
    if job['status'] not in ('queued', 'deploying'):
        raise SystemExit(f"Deployment {job['status']}: {job.get('error', '')}")
    time.sleep(2)
else:
    raise SystemExit('Timed out waiting for deployment.')
