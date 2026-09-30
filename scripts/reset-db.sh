set -u

# Tauri app_data_dir for identifier `com.roofle.app` (src-tauri/tauri.conf.json).
case "$(uname -s)" in
  Darwin)
    DATA_DIR="${ROOFLE_DATA_DIR:-$HOME/Library/Application Support/com.roofle.app}"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    DATA_DIR="${ROOFLE_DATA_DIR:-${APPDATA:-$HOME/AppData/Roaming}/com.roofle.app}"
    ;;
  *)
    DATA_DIR="${ROOFLE_DATA_DIR:-$HOME/.local/share/com.roofle.app}"
    ;;
esac

DB="$DATA_DIR/roofle.db"

if [ ! -e "$DB" ]; then
  echo "No database at $DB (nothing to reset)."
  exit 0
fi

# Remove WAL/SHM sidecars too so the next open starts from a clean file.
for f in "$DB" "$DB-wal" "$DB-shm"; do
  if [ -e "$f" ]; then
    rm -f "$f" && echo "deleted: $f"
  fi
done

echo "Done. Tables are recreated from scratch on next app launch."
