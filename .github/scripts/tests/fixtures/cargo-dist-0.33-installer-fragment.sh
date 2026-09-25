# Relevant blocks from the cargo-dist 0.33.0 generated shell installer.
verify_checksum() {
    local _file="$1"
    local _checksum_style="$2"
    local _checksum_value="$3"
    local _calculated_checksum

    if [ -z "$_checksum_value" ]; then
        return 0
    fi
    case "$_checksum_style" in
        sha256)
            if ! check_cmd sha256sum; then
                say "skipping sha256 checksum verification (it requires the 'sha256sum' command)"
                return 0
            fi
            _calculated_checksum="$(sha256sum -b "$_file" | awk '{printf $1}')"
            ;;
    esac
    if [ "$_calculated_checksum" != "$_checksum_value" ]; then
        err "checksum mismatch"
    fi
}

write_receipt() {
            echo "$RECEIPT" > "$RECEIPT_HOME/$APP_NAME-receipt.json"
            # shellcheck disable=SC2320
            local _retval=$?
    return "$_retval"
}
