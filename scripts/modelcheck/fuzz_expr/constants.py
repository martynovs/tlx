"""Shared constants of the expression fuzzer.

``TLX`` is the built binary the runner executes; ``TLC_SEM`` bounds the
concurrent TLC JVMs (at most two).
"""
import os
import threading

from tlxlib import ROOT, FAST_PROFILE

TLX = os.path.join(ROOT, "target", FAST_PROFILE, "tlx")
EXTENDS = "Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds"
MVS = ["m1", "m2", "m3"]
STRS = ["a", "b", "c", "x", "", "ab"]
FIELDS = ["a", "b", "c"]
INT_MAX = 2147483647
TLC_SEM = threading.Semaphore(2)
