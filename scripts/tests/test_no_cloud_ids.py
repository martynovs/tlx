#!/usr/bin/env python3
"""No personal cloud identities in tracked files: account IDs, RAM/CAM ARNs,
instance / security group / VPC IDs, email addresses. The real identities
belong in the git-ignored local cloud config (cloud.cloud_common.cloud_identity,
docs/cloud-verification.md).

usage: python3 scripts/tests/test_no_cloud_ids.py [-v]
"""
import os, re, subprocess, sys, unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from tlxlib import ROOT

# Placeholder identities used by the docs and the tests.
PLACEHOLDERS = {"123456789012", "210987654321", "1234567890123456", "100000000001", "100000000002", "00000000"}
# Numbers shorter than this are obviously fake (`qcs::cam:1:uin/2`).
MIN_DIGITS = 6
PATTERNS = [
    ("AWS ARN account", re.compile(r"arn:aws[\w-]*:[\w-]*:[\w-]*:(\d+)")),
    ("Aliyun RAM ARN", re.compile(r"acs:ram::(\d+)")),
    ("Tencent CAM ARN", re.compile(r"qcs::cam:(\d+)")),
    ("Tencent UIN", re.compile(r"\buin/(\d+)")),
    ("account ID", re.compile(r"(?i)(?:account|owner|uin|账号|子用户)\W{0,12}(?:id\W{0,5})?(\d+)")),
    ("Tencent instance ID", re.compile(r"\bins-([0-9a-z]{8})\b")),
    ("EC2 instance ID", re.compile(r"\bi-(0[0-9a-f]{7,16})\b")),
    ("security group ID", re.compile(r"\bsg-([0-9a-z]{8,17})\b")),
    ("VPC ID", re.compile(r"\bvpc-([0-9a-z]{8,17})\b")),
    ("email address", re.compile(r"[\w.%+-]+@([\w-]+(?:\.[\w-]+)*\.[a-z]{2,})\b")),
]
ALLOWED_EMAIL_DOMAINS = {"example.com", "example.org", "example.net"}


def allowed(kind, value):
    if kind == "email address":
        return value in ALLOWED_EMAIL_DOMAINS
    if value in PLACEHOLDERS:
        return True
    return value.isdigit() and len(value) < MIN_DIGITS


def findings(name, text):
    out = []
    for lineno, line in enumerate(text.splitlines(), 1):
        for kind, rx in PATTERNS:
            for m in rx.finditer(line):
                if not allowed(kind, m.group(1)):
                    out.append(f"{name}:{lineno}: {kind}: {m.group(0)}")
    return out


def tracked_files():
    p = subprocess.run(["git", "-C", ROOT, "ls-files", "-z"], capture_output=True, text=True)
    if p.returncode != 0:
        return None
    return [f for f in p.stdout.split("\0") if f]


class NoCloudIds(unittest.TestCase):
    def test_patterns(self):
        bad = ["account 987654321098", '"Account": "987654321098"', "arn:aws:iam::987654321098:user/x",
               "acs:ram::9876543210987654:user/tlx", "qcs::cam:987654321:uin/987654322", "主账号 987654321",
               "ins-ab12cd34", "i-0123456789abcdef0", "sg-ab12cd34", "vpc-ab12cd34ef", "someone@mail.example.cn"]
        good = ['{"account": "123456789012"}', "arn:aws:iam::123456789012:user/tlx", "acs:ram::1234567890123456:user/tlx",
                "qcs::cam:100000000001:uin/100000000002", "qcs::cam:1:uin/2", "ins-00000000", "sg-1", "vpc-1",
                "ins-1", "user@example.com", "account 12 (two)"]
        for s in bad:
            self.assertTrue(findings("x", s), s)
        for s in good:
            self.assertEqual(findings("x", s), [], s)

    def test_tracked_files_have_no_cloud_ids(self):
        files = tracked_files()
        if files is None:
            self.skipTest("not a git checkout")
        found = []
        for name in files:
            path = os.path.join(ROOT, name)
            if name == "scripts/tests/test_no_cloud_ids.py" or not os.path.isfile(path):
                continue
            try:
                with open(path, encoding="utf-8") as f:
                    text = f.read()
            except (UnicodeDecodeError, OSError):
                continue
            found += findings(name, text)
        self.assertEqual(found, [], "personal cloud identities in tracked files (move them to the local cloud config, "
                                    "docs/cloud-verification.md):\n" + "\n".join(found))


if __name__ == "__main__":
    unittest.main()
