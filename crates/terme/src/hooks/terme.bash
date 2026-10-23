# terme shell integration for bash. terme loads this itself; there is no
# need to source it by hand.
#
# It reports where each prompt ends (OSC 133 marks), so terme knows where the
# command line starts, when a command runs, and when the shell is back at a
# prompt.

if [[ -n "${__terme_hooked-}" ]]; then
  return
fi
__terme_hooked=1

__terme_prompt_start() {
  local ec=$?
  printf '\e]133;D;%s\a' "$ec"
  return "$ec"
}

__terme_prompt_end() {
  local ec=$?
  # Prompt tools such as starship rebuild PS1 every time, so mark it again
  # whenever the marks are missing.
  case "$PS1" in
    *'133;B'*) ;;
    *) PS1='\[\e]133;A\a\]'"$PS1"'\[\e]133;B\a\]' ;;
  esac
  return "$ec"
}

if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
  PROMPT_COMMAND=(__terme_prompt_start "${PROMPT_COMMAND[@]}" __terme_prompt_end)
else
  # Newlines, not semicolons: the existing value may already end with one.
  PROMPT_COMMAND=__terme_prompt_start$'\n'"${PROMPT_COMMAND-}"$'\n'__terme_prompt_end
fi

PS0="${PS0-}"$'\e]133;C\a'
