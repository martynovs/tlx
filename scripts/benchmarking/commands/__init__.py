"""The bench subcommand passes, one module per pass.

A package marker like its parent's: importing ``benchmarking.commands.run``
executes this file, and the harness's recursive discovery, the cloud upload and
the gate's path readers treat a directory an import names as a package by its
marker rather than by walking whatever happens to hold modules (HARNESS-13).
"""
