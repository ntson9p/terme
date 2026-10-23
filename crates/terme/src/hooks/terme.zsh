# terme shell integration for zsh. terme loads this itself; there is no need
# to source it by hand.
#
# It reports where each prompt ends (OSC 133 marks) and, on every redraw of
# the command line, the exact line and cursor, through a private sequence
# that terme removes before it reaches the terminal.

[[ -n ${__terme_hooked-} ]] && return
typeset -g __terme_hooked=1

__terme_precmd() {
  local ec=$?
  print -n "\e]133;D;$ec\a"
  # Prompt themes rebuild PS1 in their own precmd; this one runs after them.
  if [[ $PS1 != *'133;B'* ]]; then
    PS1=$'%{\e]133;A\a%}'"$PS1"$'%{\e]133;B\a%}'
  fi
  return $ec
}

__terme_preexec() {
  print -n "\e]133;C\a"
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __terme_precmd
add-zsh-hook preexec __terme_preexec

__terme_report_line() {
  # ESC and BEL would end the message early; they never matter for suggestions.
  local line=${BUFFER//$'\e'/ }
  line=${line//$'\a'/ }
  print -rn -- $'\e]6973;line;'"${CURSOR};${line}"$'\a'
}

__terme_report_keymap() {
  print -rn -- $'\e]6973;keymap;'"${KEYMAP}"$'\a'
}

autoload -Uz add-zle-hook-widget
add-zle-hook-widget line-init __terme_report_line
add-zle-hook-widget line-pre-redraw __terme_report_line
add-zle-hook-widget keymap-select __terme_report_keymap
