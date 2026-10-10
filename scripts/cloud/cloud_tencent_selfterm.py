#!/usr/bin/env python3
"""The in-instance half of the Tencent Cloud dead-man's switch
(scripts/cloud/cloud_tencent.py): runs ON the spot instance, never locally.

usage: cloud_tencent_selfterm.py check|terminate ROLE

The instance is launched with the CAM role ROLE (RunInstances CamRoleName);
the metadata service hands out temporary credentials of that role. The user
data installs this file and a systemd timer that runs `terminate` after
--max-minutes: TerminateInstances on the instance itself, signed (TC3-HMAC-
SHA256) with those credentials, retried until it is accepted. No long-lived
secret is ever on the instance. `check` (run over ssh right after boot)
proves that the credentials can be fetched and that a request signed with
them is authenticated: TerminateInstances of a nonexistent instance id must
come back with InvalidInstanceId.NotFound (or a permission refusal, when the
role's tag condition is evaluated first); exit status 1 otherwise (e.g.
AuthFailure.SignatureFailure, no role credentials).

Standard library only (the image's python3); no secret is printed.
"""
import datetime, hashlib, hmac, json, sys, time, urllib.error, urllib.request

METADATA = "http://metadata.tencentyun.com/latest/meta-data/"
SERVICE, VERSION = "cvm", "2017-03-12"
HOSTS = ["cvm.tencentcloudapi.com", "cvm.internal.tencentcloudapi.com"]
# The `check` request and the answers that prove valid credentials and signature.
CHECK_ID = "ins-00000000"
CHECK_OK = ("InvalidInstanceId", "AuthFailure.UnauthorizedOperation", "UnauthorizedOperation")


def metadata(path):
    with urllib.request.urlopen(METADATA + path, timeout=5) as r:
        return r.read().decode().strip()


def credentials(role):
    c = json.loads(metadata("cam/security-credentials/" + role))
    if c.get("Code") != "Success":
        raise RuntimeError(f"metadata credentials of role {role}: Code {c.get('Code')}")
    return c["TmpSecretId"], c["TmpSecretKey"], c["Token"]


def _hmac(key, msg):
    return hmac.new(key, msg.encode(), hashlib.sha256).digest()


def sign(secret_id, secret_key, host, action, region, payload, timestamp, token=""):
    """The headers of a TC3-HMAC-SHA256 signed API 3.0 POST request (JSON body `payload`)."""
    date = datetime.datetime.fromtimestamp(timestamp, datetime.timezone.utc).strftime("%Y-%m-%d")
    ct = "application/json; charset=utf-8"
    canonical = ("POST\n/\n\n" f"content-type:{ct}\nhost:{host}\n\n" "content-type;host\n"
                 + hashlib.sha256(payload.encode()).hexdigest())
    scope = f"{date}/{SERVICE}/tc3_request"
    to_sign = f"TC3-HMAC-SHA256\n{timestamp}\n{scope}\n" + hashlib.sha256(canonical.encode()).hexdigest()
    key = _hmac(_hmac(_hmac(("TC3" + secret_key).encode(), date), SERVICE), "tc3_request")
    signature = hmac.new(key, to_sign.encode(), hashlib.sha256).hexdigest()
    h = {"Authorization": f"TC3-HMAC-SHA256 Credential={secret_id}/{scope}, SignedHeaders=content-type;host, Signature={signature}",
         "Content-Type": ct, "Host": host, "X-TC-Action": action, "X-TC-Timestamp": str(timestamp),
         "X-TC-Version": VERSION, "X-TC-Region": region}
    if token:
        h["X-TC-Token"] = token
    return h


def call(cred, action, region, body):
    """The API's Response object (with an Error member on refusal); tries
    the public endpoint, then the internal one."""
    payload = json.dumps(body, separators=(",", ":"))
    last = None
    for host in HOSTS:
        req = urllib.request.Request(f"https://{host}/", data=payload.encode(), method="POST",
                                     headers=sign(cred[0], cred[1], host, action, region, payload, int(time.time()), cred[2]))
        try:
            with urllib.request.urlopen(req, timeout=15) as r:
                return json.loads(r.read().decode())["Response"]
        except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
            last = e
    raise RuntimeError(f"no API endpoint answered: {last}")


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("check", "terminate"):
        sys.exit(__doc__)
    mode, role = sys.argv[1:]
    iid, region = metadata("instance-id"), metadata("placement/region")
    if mode == "check":
        try:
            r = call(credentials(role), "TerminateInstances", region, {"InstanceIds": [CHECK_ID]})
        except Exception as e:  # noqa: BLE001 - e.g. HTTP 404: the instance has no such role
            sys.exit(f"selfterm check failed: credentials of role {role}: {e}")
        code = (r.get("Error") or {}).get("Code", "")
        if not code.startswith(CHECK_OK):
            sys.exit(f"selfterm check failed: {code or 'no error'}: {(r.get('Error') or {}).get('Message')}")
        print(f"selfterm check ok: role {role}, {iid} in {region}, signed request authenticated ({code})")
        return
    for attempt in range(1, 61):
        try:
            # Fresh credentials each time (they rotate).
            r = call(credentials(role), "TerminateInstances", region, {"InstanceIds": [iid]})
            err = r.get("Error")
            if not err:
                print(f"selfterm: TerminateInstances {iid} accepted", flush=True)
                return
            if "NotFound" in err.get("Code", ""):
                print(f"selfterm: {iid} is gone ({err['Code']})", flush=True)
                return
            print(f"selfterm: attempt {attempt}: {err.get('Code')}: {err.get('Message')}", flush=True)
        except Exception as e:  # noqa: BLE001 - keep trying until the shutdown fallback
            print(f"selfterm: attempt {attempt}: {e}", flush=True)
        time.sleep(30)
    sys.exit(1)


if __name__ == "__main__":
    main()
