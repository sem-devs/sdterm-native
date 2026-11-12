# sdterm: run your own .zprofile (login shells).
_sdterm_home="${SDTERM_ZDOTDIR_ORIG:-$HOME}"
_sdterm_dir="$ZDOTDIR"
if [[ -r "$_sdterm_home/.zprofile" ]]; then
  ZDOTDIR="$_sdterm_home"
  source "$_sdterm_home/.zprofile"
  ZDOTDIR="$_sdterm_dir"
fi
unset _sdterm_home _sdterm_dir
