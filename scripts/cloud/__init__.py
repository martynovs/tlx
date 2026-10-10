"""Cloud-provider harness and remote verification.

Package marker. The harness discovers modules recursively (HARNESS-13): making the
directory a package lets the mode lint, the cloud upload, staged-test discovery and
the verify overlay find the modules that move in here without changing that glue.
"""
