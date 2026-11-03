#!/bin/bash
# Reference fix: correct the one misspelled word.
set -euo pipefail
cd /app
sed -i 's/was recieved and/was received and/' src/messages.js
