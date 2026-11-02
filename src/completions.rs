use crate::config::Shell;

pub fn script(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => BASH,
        Shell::Zsh => ZSH,
        Shell::Fish => FISH,
    }
}

const BASH: &str = r#"_lserve_complete_files() {
    compopt -o filenames 2>/dev/null
    mapfile -t COMPREPLY < <(compgen -f -- "$1")
}

_lserve_complete_dirs() {
    compopt -o filenames 2>/dev/null
    mapfile -t COMPREPLY < <(compgen -d -- "$1")
}

_lserve() {
    local cur prev first root_seen i word route_lhs route_rhs proto
    local serve_opts='--bind --http-port --https-port -p --http-only --no-symlinks --no-markdown --markdown-theme --access-log --error-log --log-max-size --log-generations --verbose-errors --route --cert --key --help --version'
    local commands='ca cert flush config completions'

    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    first="${COMP_WORDS[1]-}"

    # Subcommands own their argument grammar. Shell redirection is deliberately
    # not suggested; users can still type it normally when they want it.
    case "$first" in
        ca)
            if [[ "$prev" == export ]]; then
                _lserve_complete_files "$cur"
                return 0
            fi
            if [[ $COMP_CWORD -eq 2 ]]; then
                COMPREPLY=( $(compgen -W 'generate export' -- "$cur") )
                return 0
            fi
            if [[ "${COMP_WORDS[2]-}" == generate ]]; then
                COMPREPLY=( $(compgen -W '--force' -- "$cur") )
            fi
            return 0
            ;;
        cert)
            if [[ $COMP_CWORD -eq 2 ]]; then
                COMPREPLY=( $(compgen -W 'generate' -- "$cur") )
                return 0
            fi
            if [[ "${COMP_WORDS[2]-}" == generate ]]; then
                COMPREPLY=( $(compgen -W '--force' -- "$cur") )
            fi
            return 0
            ;;
        flush)
            if [[ $COMP_CWORD -eq 2 ]]; then
                COMPREPLY=( $(compgen -W 'all ca cert' -- "$cur") )
            fi
            return 0
            ;;
        config)
            if [[ "$prev" == --write ]]; then
                _lserve_complete_files "$cur"
                return 0
            fi
            if [[ $COMP_CWORD -eq 2 ]]; then
                COMPREPLY=( $(compgen -W '--write' -- "$cur") )
            fi
            return 0
            ;;
        completions)
            if [[ $COMP_CWORD -eq 2 ]]; then
                COMPREPLY=( $(compgen -W 'bash zsh fish' -- "$cur") )
            fi
            return 0
            ;;
    esac

    # Value completion for serve options.
    case "$prev" in
        --markdown-theme)
            COMPREPLY=( $(compgen -W 'auto light dark' -- "$cur") )
            return 0
            ;;
        --cert|--key|--access-log|--error-log)
            _lserve_complete_files "$cur"
            return 0
            ;;
        --route)
            if [[ "$cur" == *=* ]]; then
                route_lhs="${cur%%=*}="
                route_rhs="${cur#*=}"
                proto="$(compgen -W 'http://' -- "$route_rhs")"
                if [[ -n "$proto" ]]; then
                    COMPREPLY=( "${route_lhs}${proto}" )
                fi
            else
                COMPREPLY=( 'HOST=http://' )
            fi
            return 0
            ;;
        --bind|--http-port|--https-port|-p|--log-max-size|--log-generations)
            # These values are typed, not filesystem paths.
            return 0
            ;;
        --)
            _lserve_complete_dirs "$cur"
            return 0
            ;;
    esac

    if [[ "$cur" == -* ]]; then
        COMPREPLY=( $(compgen -W "$serve_opts" -- "$cur") )
        return 0
    fi

    # Detect whether the optional document-root positional has already been
    # consumed. Options that take a value consume the following word.
    root_seen=0
    i=1
    while (( i < COMP_CWORD )); do
        word="${COMP_WORDS[i]}"
        case "$word" in
            --bind|--http-port|--https-port|-p|--markdown-theme|--access-log|--error-log|--log-max-size|--log-generations|--route|--cert|--key)
                ((i += 2))
                continue
                ;;
            --http-only|--no-symlinks|--no-markdown|--verbose-errors|-h|--help|-V|--version)
                ((i += 1))
                continue
                ;;
            --)
                if (( i + 1 < COMP_CWORD )); then
                    root_seen=1
                fi
                break
                ;;
            -*)
                ((i += 1))
                continue
                ;;
            *)
                root_seen=1
                break
                ;;
        esac
    done

    if (( root_seen == 0 )); then
        local -a dirs
        mapfile -t dirs < <(compgen -d -- "$cur")
        compopt -o filenames 2>/dev/null
        if [[ $COMP_CWORD -eq 1 ]]; then
            COMPREPLY=( $(compgen -W "$commands $serve_opts" -- "$cur") "${dirs[@]}" )
        else
            COMPREPLY=( $(compgen -W "$serve_opts" -- "$cur") "${dirs[@]}" )
        fi
        return 0
    fi

    # Once the document root exists, only options remain in serve mode.
    COMPREPLY=( $(compgen -W "$serve_opts" -- "$cur") )
}
complete -F _lserve lserve
"#;

const ZSH: &str = r#"#compdef lserve
_lserve() {
  _arguments -C \
    '1:document root or command:_alternative "commands:(ca cert flush config completions)" "directories:_directories"' \
    '--bind[bind to loopback/private address]:address:' \
    '--http-port[HTTP port]:port:' \
    '(-p)--https-port[HTTPS port]:port:' \
    '(--https-port)-p[HTTPS port]:port:' \
    '--http-only[serve HTTP directly]' \
    '--no-symlinks[do not follow symlinks]' \
    '--no-markdown[serve Markdown raw]' \
    '--markdown-theme[Markdown theme]:theme:(auto light dark)' \
    '--access-log[access log path]:file:_files' \
    '--error-log[error log path]:file:_files' \
    '--log-max-size[rotation threshold]:size:' \
    '--log-generations[rotated files to retain]:count:' \
    '--verbose-errors[show limited internal 5xx detail]' \
    '*--route[Host/path proxy route]:HOST/PATH=http\://:' \
    '--cert[PEM certificate]:file:_files' \
    '--key[PEM private key]:file:_files' \
    '(-h --help)'{-h,--help}'[show help]' \
    '(-V --version)'{-V,--version}'[show version]'
}
_lserve "$@"
"#;

const FISH: &str = r#"complete -c lserve -f
complete -c lserve -n '__fish_use_subcommand' -a 'ca cert flush config completions'
complete -c lserve -l bind -r -d 'Bind to loopback/private address'
complete -c lserve -l http-port -r -d 'HTTP port'
complete -c lserve -l https-port -s p -r -d 'HTTPS port'
complete -c lserve -l http-only -d 'Serve HTTP directly'
complete -c lserve -l no-symlinks -d 'Do not follow symlinks'
complete -c lserve -l no-markdown -d 'Serve Markdown raw'
complete -c lserve -l markdown-theme -r -a 'auto light dark' -d 'Markdown theme'
complete -c lserve -l access-log -r -d 'Access log path'
complete -c lserve -l error-log -r -d 'Error log path'
complete -c lserve -l log-max-size -r -d 'Rotation threshold'
complete -c lserve -l log-generations -r -d 'Rotated files retained'
complete -c lserve -l verbose-errors -d 'Show limited internal 5xx detail'
complete -c lserve -l route -r -d 'Host/path proxy route HOST[/PATH]=http://'
complete -c lserve -l cert -r -d 'PEM certificate'
complete -c lserve -l key -r -d 'PEM private key'
complete -c lserve -s h -l help -d 'Show help'
complete -c lserve -s V -l version -d 'Show version'
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_completion_tracks_cli_grammar() {
        assert!(BASH.contains("generate export"));
        assert!(BASH.contains("all ca cert"));
        assert!(BASH.contains("HOST=http://"));
        assert!(BASH.contains("auto light dark"));
        assert!(!BASH.contains("--export"));
    }
}
