# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.
#
# The Phase 8 live checklist's files and folders on QS/400, made by the
# system's own commands: a Qshell script, run as QSECOFR (or a profile with
# *ALLOBJ and *SECADM) after the Phase 7 setup has made profiles BOB and
# ANN:
#
#   QSH CMD('/home/QSECOFR/ifs-setup.sh')
#
# Content is written with printf (a file Qshell makes is tagged 1208); the
# rest is CL through `system`: MKDIR, CPY with TOCCSID for the converted
# copies, ADDLNK, CHGAUT, and CHKOUT submitted as ANN; owners are set with
# Qshell's chown -R, since CHGOWN takes one object. The script is
# ASCII only (other characters are printf escapes), so it reads the same
# whatever CCSID the copy on the system is tagged with.
#
# It makes, under /home/BOB/qsa8 (owned by BOB):
#   many/            1,200 files f0000.txt to f1199.txt, for paging
#   text/            the same text in CCSIDs 1208, 37, 273 and 819, Japanese
#                    in mixed 939, and binary.dat tagged 65535
#   link1208         a link to text/text1208.txt; broken, a link to gone.txt
#   edit37.txt       a CCSID 37 text file to edit and save
#   checkedout.txt   checked out to ANN
#   tree/            docs, docs/old, src, src/lib and six files, with
#                    src/lib/m.txt checked out to ANN
# and /home/ANN/qsa8private, which BOB may not read.
#
# To run it again: as ANN, CHKIN both checked-out files; then, as QSECOFR,
# RD DIR('/home/BOB/qsa8') SUBTREE(*ALL) and
# RD DIR('/home/ANN/qsa8private') SUBTREE(*ALL).

set -e
B=/home/BOB/qsa8
P=/home/ANN/qsa8private

cl() {
    echo "> $1"
    system "$1"
}

cl "MKDIR DIR('$B')"
for d in many text tree tree/docs tree/docs/old tree/src tree/src/lib; do
    cl "MKDIR DIR('$B/$d')"
done

# 1,200 small files.
i=0
while [ $i -lt 1200 ]; do
    printf 'file %d\n' $i > "$B/many/f$(printf %04d $i).txt"
    i=$((i + 1))
done
echo "1200 files in $B/many"

# The text of QS/400's test system, in UTF-8; the system's CPY converts it.
printf 'Gr\303\274\303\237e aus Z\303\274rich: 1 + 1 = 2, \302\241ol\303\251! [ok] {x} \\ ~\nZweite Zeile - \303\244\303\266\303\274 \303\204\303\226\303\234 \303\237.\n' > "$B/text/text1208.txt"
setccsid 1208 "$B/text/text1208.txt"
for c in 37 273 819; do
    cl "CPY OBJ('$B/text/text1208.txt') TOOBJ('$B/text/text$c.txt') TOCCSID($c) DTAFMT(*TEXT)"
done
printf 'QS/400 \343\201\257\346\227\245\346\234\254\350\252\236\343\202\222\345\244\211\346\217\233\343\201\227\343\201\276\343\201\231\343\200\202\n\357\275\266\357\276\200\357\275\266\357\276\205 ABC 123 \346\235\261\344\272\254\351\203\275\343\200\200\345\215\203\344\273\243\347\224\260\345\214\272\nabc\343\201\202x \347\265\202\343\202\217\343\202\212\n' > "$B/text/jp1208.txt"
setccsid 1208 "$B/text/jp1208.txt"
cl "CPY OBJ('$B/text/jp1208.txt') TOOBJ('$B/text/text939.txt') TOCCSID(939) DTAFMT(*TEXT)"
printf '\000\016\377\017\101\200\301' > "$B/text/binary.dat"
setccsid 65535 "$B/text/binary.dat"
cl "CPY OBJ('$B/text/text37.txt') TOOBJ('$B/edit37.txt')"

# Links: one to a file, one to nothing.
cl "ADDLNK OBJ('$B/text/text1208.txt') NEWLNK('$B/link1208') LNKTYPE(*SYMBOLIC)"
cl "ADDLNK OBJ('$B/gone.txt') NEWLNK('$B/broken') LNKTYPE(*SYMBOLIC)"

# The tree: a depth-first delete stops at src/lib/m.txt.
for f in readme.txt docs/guide.txt docs/old/notes.txt src/main.txt src/lib/a.txt src/lib/z.txt; do
    printf '%s\n' "$B/tree/$f" > "$B/tree/$f"
done
printf 'checked out to ANN\n' > "$B/tree/src/lib/m.txt"
printf 'checked out to ANN\n' > "$B/checkedout.txt"

# BOB owns it all; ANN may change the two files she checks out.
chown -R BOB "$B"
for f in "$B/checkedout.txt" "$B/tree/src/lib/m.txt"; do
    cl "CHGAUT OBJ('$f') USER(ANN) DTAAUT(*RWX) OBJAUT(*ALL)"
    cl "SBMJOB CMD(CHKOUT OBJ('$f')) JOB(QSA8CHKOUT) USER(ANN)"
done

# ANN's folder that BOB may not read.
cl "MKDIR DIR('$P')"
printf "ANN's only\n" > "$P/secret.txt"
chown -R ANN "$P"
cl "CHGAUT OBJ('$P') USER(*PUBLIC) DTAAUT(*EXCLUDE) OBJAUT(*NONE)"

echo "Done. Check WRKLNK '$B/*' and that both CHKOUT jobs ended normally."
