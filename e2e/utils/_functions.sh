# Log a formatted message to stderr.
log() {
    echo -e "* [$(date +"%Y-%m-%d %H:%M:%S")] $@" 1>&2;
}
