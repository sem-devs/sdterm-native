# sdterm: run your own .zshrc, then add command suggestions and Tab completion if you have none.
_sdterm_home="${SDTERM_ZDOTDIR_ORIG:-$HOME}"
_sdterm_dir="$ZDOTDIR"

# macOS /etc/zshrc keeps the history next to ZDOTDIR, which is this folder. Put it back where it was.
[[ "$HISTFILE" == "$_sdterm_dir/.zsh_history" ]] && HISTFILE="$_sdterm_home/.zsh_history"

if [[ -r "$_sdterm_home/.zshrc" ]]; then
  ZDOTDIR="$_sdterm_home"
  source "$_sdterm_home/.zshrc"
  ZDOTDIR="$_sdterm_dir"
fi

if [[ -n "$SDTERM_SUGGESTIONS" && -o interactive ]]; then
  # Many terminals at once: write each command to the history when it is entered, so the others
  # can suggest it straight away. Keep enough history for the suggestions to be useful.
  [[ -o sharehistory || -o incappendhistory || -o incappendhistorytime ]] || setopt incappendhistory
  (( ${HISTSIZE:-0} < 20000 )) && HISTSIZE=20000
  (( ${SAVEHIST:-0} < 20000 )) && SAVEHIST=20000

  # Tab completion for git branches, npm scripts, ssh hosts and so on, unless already set up.
  if (( ! $+functions[compdef] )); then
    autoload -Uz compinit
    compinit -i -d "${SDTERM_STATE_DIR:-$_sdterm_home}/.zcompdump"
    zstyle ':completion:*' menu select
    zstyle ':completion:*' matcher-list 'm:{a-z}={A-Z}'
  fi

  # A grey suggestion from commands you ran before; the right arrow accepts it.
  if (( ! $+functions[_zsh_autosuggest_start] )) && [[ -r "$SDTERM_SHELL_DIR/zsh-autosuggestions.zsh" ]]; then
    source "$SDTERM_SHELL_DIR/zsh-autosuggestions.zsh"
  fi
fi

# Back to the user's own startup folder, so shells started from here behave as usual.
if [[ -n "$SDTERM_ZDOTDIR_ORIG" ]]; then ZDOTDIR="$SDTERM_ZDOTDIR_ORIG"; else unset ZDOTDIR; fi
unset _sdterm_home _sdterm_dir SDTERM_ZDOTDIR_ORIG SDTERM_SHELL_DIR SDTERM_STATE_DIR SDTERM_SUGGESTIONS
