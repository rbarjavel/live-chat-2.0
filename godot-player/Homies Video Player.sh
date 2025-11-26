#!/bin/sh
printf '\033c\033]0;%s\a' Homies Video Player
base_path="$(dirname "$(realpath "$0")")"
"$base_path/Homies Video Player.x86_64" "$@"
