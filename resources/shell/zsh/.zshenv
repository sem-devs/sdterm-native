# sdterm: run your own .zshenv, then carry on. ZDOTDIR points at this folder only while zsh starts up.
_sdterm_home="${SDTERM_ZDOTDIR_ORIG:-$HOME}"
_sdterm_dir="$ZDOTDIR"
if [[ -r "$_sdterm_home/.zshenv" ]]; then
  ZDOTDIR="$_sdterm_home"
  source "$_sdterm_home/.zshenv"
  # A .zshenv that moves ZDOTDIR itself says where the rest of the user's files are.
  [[ "$ZDOTDIR" != "$_sdterm_home" && -n "$ZDOTDIR" ]] && SDTERM_ZDOTDIR_ORIG="$ZDOTDIR"
  ZDOTDIR="$_sdterm_dir"
fi
unset _sdterm_home _sdterm_dir
