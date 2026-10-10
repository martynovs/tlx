---- MODULE O3 ----
\* TLC prints the variables in the order of SANY's symbol table (a
\* java.util.Hashtable): with 144 symbols (72 built-in) as aa, et.
EXTENDS Naturals
VARIABLES aa, et
Init == aa = 0 /\ et = 0
Next == aa' = aa + 1 /\ et' = et
F0 == 0
F1 == 1
F2 == 2
F3 == 3
F4 == 4
F5 == 5
F6 == 6
F7 == 7
F8 == 8
F9 == 9
F10 == 10
F11 == 11
F12 == 12
F13 == 13
F14 == 14
F15 == 15
F16 == 16
F17 == 17
F18 == 18
F19 == 19
F20 == 20
F21 == 21
F22 == 22
F23 == 23
F24 == 24
F25 == 25
F26 == 26
F27 == 27
F28 == 28
F29 == 29
F30 == 30
F31 == 31
F32 == 32
F33 == 33
F34 == 34
F35 == 35
F36 == 36
F37 == 37
F38 == 38
F39 == 39
F40 == 40
F41 == 41
F42 == 42
F43 == 43
F44 == 44
F45 == 45
F46 == 46
F47 == 47
F48 == 48
F49 == 49
F50 == 50
F51 == 51
F52 == 52
F53 == 53
F54 == 54
Inv == aa < 1
====
