# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.
#
# The Phase 9 live checklist's program and authority on QS/400, made by the
# system's own commands: a Qshell script, run as QSECOFR (or a profile with
# *ALLOBJ and *SECADM) after the Phase 7 setup has made profiles BOB and
# ANN:
#
#   QSH CMD('/home/QSECOFR/msgs-setup.sh')
#
# It makes:
#   QGPL/QSA9FAIL   an ILE CL program whose DLTF of a file that is not there
#                   nobody monitors, so the program ends with CEE9901 (reply
#                   C, D, F, G, I or R); submitted with INQMSGRPY(*RQD) the
#                   job sends it to QSYSOPR and waits in MSGW for the reply;
#   QSYS/QSYSOPR    ANN excluded (*EXCLUDE), so ANN may not display its
#                   messages, while BOB keeps the public authority.
#
# To run it again: DLTPGM PGM(QGPL/QSA9FAIL); to give ANN back QSYSOPR:
# RVKOBJAUT OBJ(QSYS/QSYSOPR) OBJTYPE(*MSGQ) USER(ANN) AUT(*ALL).

set -e
S=/home/QSECOFR/qsa9fail.clle

cl() {
    echo "> $1"
    system "$1"
}

# The source, ASCII only, tagged 1208 as Qshell makes it.
printf '%s\n' \
    "/* Ends with CEE9901: the DLTF's CPF2105 is not monitored. */" \
    "             PGM" \
    "             DLTF       FILE(QGPL/QSA9NOPE)" \
    "             ENDPGM" > "$S"
cl "CRTBNDCL PGM(QGPL/QSA9FAIL) SRCSTMF('$S') REPLACE(*YES)"
cl "GRTOBJAUT OBJ(QSYS/QSYSOPR) OBJTYPE(*MSGQ) USER(ANN) AUT(*EXCLUDE)"
echo "QGPL/QSA9FAIL made; ANN excluded from QSYSOPR"
