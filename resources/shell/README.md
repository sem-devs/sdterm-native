# Shell integration (shared with the Electron app)

Files sdterm hands to the shells it starts. Nothing here is copied into your home folder and your own
`~/.zshrc` is never edited.

`zsh/` is a startup folder for zsh. zsh is pointed at it only while it starts: each file runs your own
file of the same name first, then (in `.zshrc`) adds command suggestions and Tab completion unless you
already have them. The real startup folder is put back before the first prompt.

`zsh-autosuggestions.zsh` is [zsh-autosuggestions](https://github.com/zsh-users/zsh-autosuggestions)
v0.7.1 by Thiago de Arruda and Eric Freese, MIT licensed (see `zsh-autosuggestions.LICENSE`).

It can be switched off in Settings, Terminal, "Command suggestions".
