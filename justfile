set positional-arguments
set dotenv-load := false

root := justfile_directory()
just := just_executable()

# Check local containment prerequisites and run a contained no-op.
verify-doctor:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    required=(bash jq flock systemd-run systemctl python3 make cargo rustup sha256sum tee mkfifo)
    for tool in "${required[@]}"; do
        command -v "$tool" >/dev/null || { printf 'missing required tool: %s\n' "$tool" >&2; exit 1; }
    done
    if [[ ! -r /sys/fs/cgroup/cgroup.controllers ]] || ! grep -qw memory /sys/fs/cgroup/cgroup.controllers; then
        printf 'cgroup v2 memory controller is unavailable\n' >&2
        exit 1
    fi
    printf 'just=%s\n' "$("$just_bin" --version)" >&2
    printf 'systemd=%s\n' "$(systemd-run --version | sed -n '1p')" >&2
    VERIFY_RUN_ID="doctor-${BASHPID}-${RANDOM}" "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 30 bash -c 'printf contained-preflight\\n'

# Exercise the real cgroup guard, OOM, timeout, status, lock, and cancellation paths.
verify-self-test:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    verification_dir="$root/target/verification"
    mkdir -p "$verification_dir"
    nonce="${BASHPID}-${RANDOM}"

    run_expect_failure() {
        local expected_description=$1
        shift
        set +e
        "$@"
        local status=$?
        set -e
        if (( status == 0 )); then
            printf '%s unexpectedly succeeded\n' "$expected_description" >&2
            return 1
        fi
        printf '%s failed as required (status %s)\n' "$expected_description" "$status" >&2
    }

    marker="$verification_dir/invalid-backend-$nonce"
    rm -f "$marker"
    run_expect_failure 'invalid backend' env VERIFY_SYSTEMD_MODE=invalid VERIFY_RUN_ID="selftest-invalid-$nonce" \
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 5 bash -c 'printf started > "$1"' _ "$marker"
    [[ ! -e "$marker" ]] || { printf 'invalid backend started its workload\n' >&2; exit 1; }

    run_expect_failure 'direct guard' "$just_bin" --justfile "$root/justfile" --working-directory "$root" _verify-guard 64 true

    exit_id="selftest-exit-$nonce"
    set +e
    VERIFY_RUN_ID="$exit_id" "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 5 bash -c 'exit 23'
    exit_status=$?
    set -e
    [[ $exit_status -eq 23 ]] || { printf 'exit status was %s, expected 23\n' "$exit_status" >&2; exit 1; }

    timeout_id="selftest-timeout-$nonce"
    run_expect_failure 'runtime timeout' env VERIFY_RUN_ID="$timeout_id" \
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 1 bash -c 'exec sleep 30'
    jq -e '.result == "timeout" or .result == "signal" or .result == "watchdog"' \
        "$verification_dir/runs/$timeout_id/result.json" >/dev/null || {
        printf 'timeout result was not recorded\n' >&2
        exit 1
    }

    oom_id="selftest-oom-$nonce"
    oom_probe=$'import os, subprocess, sys, time\np=subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"])\nprint(p.pid, flush=True)\nchunks=[]\nfor _ in range(128):\n    chunks.append(bytearray(1024*1024))\n    chunks[-1][0]=1\ntime.sleep(60)'
    run_expect_failure 'memory OOM' env VERIFY_MEMORY_MIB=64 VERIFY_RUN_ID="$oom_id" \
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 5 python3 -c "$oom_probe"
    jq -e '.result == "oom-kill" or .oom_kill > 0' "$verification_dir/runs/$oom_id/result.json" >/dev/null || {
        printf 'OOM evidence was not recorded\n' >&2
        exit 1
    }
    oom_child=$(grep -E '^[0-9]+$' "$verification_dir/runs/$oom_id/output.log" | tail -n 1 || true)
    if [[ -n $oom_child && -d /proc/$oom_child ]]; then
        printf 'OOM probe child %s survived\n' "$oom_child" >&2
        exit 1
    fi

    read -r -d '' bridge <<'BRIDGE' || true
    set -euo pipefail
    just_pid=0
    cancel=0
    forward() {
        cancel=$1
        if (( just_pid > 0 )); then kill -TERM "$just_pid" 2>/dev/null || true; fi
    }
    trap 'forward 130' INT
    trap 'forward 143' TERM
    "$1" --justfile "$2/justfile" --working-directory "$2" verify-run 20 bash -c 'printf "%s\n" "$BASHPID" > "$1"; exec sleep 300' _ "$3" &
    just_pid=$!
    set +e
    wait "$just_pid"
    status=$?
    set -e
    if (( cancel != 0 )); then exit "$cancel"; fi
    exit "$status"
    BRIDGE

    for signal_case in INT TERM; do
        ready="$verification_dir/cancel-${signal_case,,}-$nonce.fifo"
        rm -f "$ready"
        mkfifo "$ready"
        cancel_id="selftest-cancel-${signal_case,,}-$nonce"
        env --default-signal=INT,TERM VERIFY_RUN_ID="$cancel_id" bash -c "$bridge" _ "$just_bin" "$root" "$ready" &
        entry_pid=$!
        IFS= read -r workload_pid < "$ready"

        if [[ $signal_case == INT ]]; then
            lock_marker="$verification_dir/lock-marker-$nonce"
            rm -f "$lock_marker"
            run_expect_failure 'overlapping invocation' env VERIFY_RUN_ID="selftest-lock-$nonce" \
                "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 5 bash -c 'printf ran > "$1"' _ "$lock_marker"
            [[ ! -e $lock_marker ]] || { printf 'overlapping workload ran\n' >&2; exit 1; }
        fi

        kill -s "$signal_case" "$entry_pid"
        set +e
        wait "$entry_pid"
        cancel_status=$?
        set -e
        if [[ $signal_case == INT ]]; then expected=130; else expected=143; fi
        [[ $cancel_status -eq $expected ]] || {
            printf '%s cancellation returned %s, expected %s\n' "$signal_case" "$cancel_status" "$expected" >&2
            exit 1
        }
        unit=$(<"$verification_dir/runs/$cancel_id/unit")
        mode=${VERIFY_SYSTEMD_MODE:-user}
        if [[ $mode == user ]]; then ctl=(systemctl --user); else ctl=(sudo -n systemctl --system); fi
        state=$("${ctl[@]}" show "$unit" -p ActiveState --value 2>/dev/null || true)
        for _ in {1..200}; do
            if [[ $state != active && $state != activating && $state != deactivating && ! -d /proc/$workload_pid ]]; then
                break
            fi
            sleep 0.05
            state=$("${ctl[@]}" show "$unit" -p ActiveState --value 2>/dev/null || true)
        done
        [[ $state != active && $state != activating && $state != deactivating ]] || {
            printf '%s unit %s remained %s\n' "$signal_case" "$unit" "$state" >&2
            exit 1
        }
        [[ ! -d /proc/$workload_pid ]] || {
            printf '%s workload %s survived cancellation\n' "$signal_case" "$workload_pid" >&2
            exit 1
        }
        rm -f "$ready"
    done
    printf 'containment self-test passed\n' >&2

# Run one command in a hard-capped transient systemd service.
verify-run timeout_seconds +command:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    just_version=$("$just_bin" --version)
    if [[ ! $just_version =~ ^just[[:space:]]+([0-9]+)\.([0-9]+)\.([0-9]+) ]] ||
        (( BASH_REMATCH[1] < 1 || (BASH_REMATCH[1] == 1 && BASH_REMATCH[2] < 51) )); then
        printf 'just 1.51.0 or newer is required; found %s\n' "$just_version" >&2
        exit 1
    fi
    timeout_seconds=$1
    shift
    (( $# > 0 )) || { printf 'verify-run requires a command\n' >&2; exit 2; }
    [[ $timeout_seconds =~ ^[0-9]+$ ]] && (( timeout_seconds >= 1 && timeout_seconds <= 3600 )) || {
        printf 'timeout must be an integer in 1..=3600\n' >&2
        exit 2
    }
    memory_mib=${VERIFY_MEMORY_MIB:-4096}
    [[ $memory_mib =~ ^[0-9]+$ ]] && (( memory_mib >= 64 && memory_mib <= 4096 )) || {
        printf 'VERIFY_MEMORY_MIB must be an integer in 64..=4096\n' >&2
        exit 2
    }
    mode=${VERIFY_SYSTEMD_MODE:-user}
    [[ $mode == user || $mode == system ]] || {
        printf 'VERIFY_SYSTEMD_MODE must be user or system\n' >&2
        exit 2
    }
    for tool in systemd-run systemctl flock jq tee mkfifo; do
        command -v "$tool" >/dev/null || { printf 'missing required tool: %s\n' "$tool" >&2; exit 1; }
    done
    [[ -r /sys/fs/cgroup/cgroup.controllers ]] && grep -qw memory /sys/fs/cgroup/cgroup.controllers || {
        printf 'cgroup v2 memory controller is unavailable\n' >&2
        exit 1
    }

    requested_bytes=$((memory_mib * 1024 * 1024))
    headroom_bytes=$(((memory_mib + 1024) * 1024 * 1024))
    mem_available_kib=0
    while read -r key value _; do
        if [[ $key == MemAvailable: ]]; then mem_available_kib=$value; break; fi
    done < /proc/meminfo
    (( mem_available_kib * 1024 >= headroom_bytes )) || {
        printf 'MemAvailable is %s KiB; need at least %s MiB for cap plus headroom\n' "$mem_available_kib" "$((memory_mib + 1024))" >&2
        exit 1
    }

    uid=$(id -u)
    gid=$(id -g)
    if [[ $mode == user ]]; then
        run=(systemd-run --user)
        ctl=(systemctl --user)
        manager_cgroup=$("${ctl[@]}" show -p ControlGroup --value)
        slice_cgroup=$("${ctl[@]}" show app.slice -p ControlGroup --value)
    else
        sudo -n true
        run=(sudo -n systemd-run --system --uid="$uid" --gid="$gid")
        ctl=(sudo -n systemctl --system)
        manager_cgroup=/
        slice_cgroup=$("${ctl[@]}" show system.slice -p ControlGroup --value)
    fi
    [[ -n $manager_cgroup && -n $slice_cgroup ]] || {
        printf 'cannot determine destination cgroup for %s systemd manager\n' "$mode" >&2
        exit 1
    }

    declare -A checked_cgroups=()
    check_headroom() {
        local relative=$1
        while :; do
            [[ -n ${checked_cgroups[$relative]+x} ]] || {
                checked_cgroups[$relative]=1
                local directory="/sys/fs/cgroup${relative}"
                if [[ $relative == / && ( ! -r $directory/memory.max || ! -r $directory/memory.current ) ]]; then
                    :
                else
                    [[ -r $directory/memory.max && -r $directory/memory.current ]] || {
                        printf 'memory controller is not delegated at %s\n' "$relative" >&2
                        return 1
                    }
                    local maximum current remaining
                    maximum=$(<"$directory/memory.max")
                    current=$(<"$directory/memory.current")
                    if [[ $maximum != max ]]; then
                        [[ $maximum =~ ^[0-9]+$ && $current =~ ^[0-9]+$ ]] || {
                            printf 'invalid cgroup memory values at %s\n' "$relative" >&2
                            return 1
                        }
                        remaining=$((maximum - current))
                        (( remaining >= headroom_bytes )) || {
                            printf 'ancestor cgroup %s has %s bytes remaining; need %s\n' "$relative" "$remaining" "$headroom_bytes" >&2
                            return 1
                        }
                    fi
                fi
            }
            [[ $relative == / ]] && break
            relative=${relative%/*}
            [[ -n $relative ]] || relative=/
        done
    }
    check_headroom "$manager_cgroup"
    check_headroom "$slice_cgroup"

    run_id=${VERIFY_RUN_ID:-run-${BASHPID}-${RANDOM}}
    [[ $run_id =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,95}$ ]] || {
        printf 'VERIFY_RUN_ID contains unsupported characters or is too long\n' >&2
        exit 2
    }
    run_dir="$root/target/verification/runs/$run_id"
    mkdir -p "$(dirname "$run_dir")"
    mkdir "$run_dir" || { printf 'verification run id already exists: %s\n' "$run_id" >&2; exit 2; }
    unit="blend-verification-${uid}-${BASHPID}-${RANDOM}.service"
    printf '%s\n' "$unit" > "$run_dir/unit"
    printf '%q ' "$@" > "$run_dir/command.txt"
    printf '\n' >> "$run_dir/command.txt"
    jq -cn --arg mode "$mode" --argjson timeout "$timeout_seconds" --argjson memory_mib "$memory_mib" \
        --argjson memory_bytes "$requested_bytes" \
        '{mode:$mode,timeout_seconds:$timeout,memory_mib:$memory_mib,memory_bytes:$memory_bytes,swap_bytes:0,tasks_max:256,cpu_quota_percent:200}' \
        > "$run_dir/limits.json"
    {
        "$just_bin" --version
        systemd-run --version | sed -n '1p'
        cargo --version 2>/dev/null || true
        rustc --version 2>/dev/null || true
    } > "$run_dir/versions.txt"

    env_cmd=(env -i "HOME=$HOME" "PATH=$PATH")
    for name in CARGO_HOME RUSTUP_HOME KANI_HOME TMPDIR CARGO_TARGET_DIR CARGO_NET_OFFLINE \
        CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS RUSTFLAGS RUSTDOCFLAGS RUST_BACKTRACE ASAN_OPTIONS UBSAN_OPTIONS \
        RUSTUP_TOOLCHAIN VERIFY_SYSTEMD_MODE VERIFY_MEMORY_MIB; do
        if [[ -v $name ]]; then env_cmd+=("$name=${!name}"); fi
    done
    env_cmd+=("VERIFY_RUN_DIR=$run_dir")
    guarded=("$just_bin" --justfile "$root/justfile" --working-directory "$root" _verify-guard "$memory_mib" "$@")
    systemd_command=("${run[@]}" --wait --pipe --service-type=exec --expand-environment=no \
        --unit="$unit" --working-directory="$root" \
        --property=MemoryAccounting=yes --property="MemoryMax=$requested_bytes" --property=MemorySwapMax=0 \
        --property=OOMPolicy=kill --property=KillMode=control-group --property=TimeoutStopSec=10s \
        --property="RuntimeMaxSec=${timeout_seconds}s" --property=TasksMax=256 --property=CPUQuota=200% \
        "${env_cmd[@]}" "${guarded[@]}")

    fifo="$run_dir/output.fifo"
    mkfifo "$fifo"
    tee "$run_dir/output.log" < "$fifo" &
    tee_pid=$!
    systemd_pid=0
    watcher_pid=0
    cancel_status=0

    stop_unit() {
        "${ctl[@]}" stop "$unit" >> "$run_dir/cleanup.log" 2>&1 || true
    }
    on_signal() {
        cancel_status=$1
        stop_unit
        if (( systemd_pid > 0 )); then kill -TERM "$systemd_pid" 2>/dev/null || true; fi
    }
    on_exit() {
        local saved=$?
        trap - EXIT INT TERM
        stop_unit
        if (( watcher_pid > 0 )); then kill -TERM "$watcher_pid" 2>/dev/null || true; wait "$watcher_pid" 2>/dev/null || true; fi
        if (( systemd_pid > 0 )) && kill -0 "$systemd_pid" 2>/dev/null; then
            kill -TERM "$systemd_pid" 2>/dev/null || true
            wait "$systemd_pid" 2>/dev/null || true
        fi
        if kill -0 "$tee_pid" 2>/dev/null; then kill -TERM "$tee_pid" 2>/dev/null || true; fi
        wait "$tee_pid" 2>/dev/null || true
        rm -f "$fifo"
        exit "$saved"
    }
    trap 'on_signal 130' INT
    trap 'on_signal 143' TERM
    trap on_exit EXIT

    "${systemd_command[@]}" > "$fifo" 2>&1 &
    systemd_pid=$!
    watch_memory() {
        local cgroup_path events
        while kill -0 "$systemd_pid" 2>/dev/null; do
            cgroup_path=$("${ctl[@]}" show "$unit" -p ControlGroup --value 2>/dev/null || true)
            if [[ -n $cgroup_path && -r /sys/fs/cgroup$cgroup_path/memory.events ]]; then
                events=$(</sys/fs/cgroup$cgroup_path/memory.events)
                printf '%s\n' "$events" > "$run_dir/memory.events"
                printf '%s\n' "$cgroup_path" > "$run_dir/cgroup.path"
            fi
            sleep 0.05
        done
    }
    watch_memory &
    watcher_pid=$!

    set +e
    wait "$systemd_pid"
    workload_status=$?
    set -e
    if (( cancel_status != 0 )); then
        stop_unit
        if kill -0 "$systemd_pid" 2>/dev/null; then
            set +e
            wait "$systemd_pid"
            set -e
        fi
        workload_status=$cancel_status
    fi
    kill -TERM "$watcher_pid" 2>/dev/null || true
    wait "$watcher_pid" 2>/dev/null || true
    watcher_pid=0
    wait "$tee_pid" || { printf 'log streaming failed\n' >&2; exit 1; }
    tee_pid=0
    rm -f "$fifo"

    "${ctl[@]}" show "$unit" \
        -p ActiveState -p SubState -p Result -p ExecMainCode -p ExecMainStatus \
        -p MemoryCurrent -p MemoryPeak -p MemorySwapCurrent -p ControlGroup \
        > "$run_dir/systemd.properties" 2>&1 || true
    result=$(sed -n 's/^Result=//p' "$run_dir/systemd.properties" | tail -n 1)
    exec_status=$(sed -n 's/^ExecMainStatus=//p' "$run_dir/systemd.properties" | tail -n 1)
    memory_peak=$(sed -n 's/^MemoryPeak=//p' "$run_dir/systemd.properties" | tail -n 1)
    oom_kill=0
    if [[ -r $run_dir/memory.events ]]; then
        while read -r key value; do [[ $key == oom_kill ]] && oom_kill=$value; done < "$run_dir/memory.events"
    fi
    jq -cn --arg unit "$unit" --arg mode "$mode" --arg result "${result:-unknown}" \
        --argjson status "$workload_status" --arg exec_status "${exec_status:-unknown}" \
        --arg memory_peak "${memory_peak:-unknown}" --argjson oom_kill "$oom_kill" \
        '{unit:$unit,mode:$mode,status:$status,result:$result,exec_main_status:$exec_status,memory_peak:$memory_peak,oom_kill:$oom_kill}' \
        > "$run_dir/result.json"
    printf 'verification artifacts: %s\n' "$run_dir" >&2
    trap - EXIT INT TERM
    exit "$workload_status"

[private]
_verify-guard memory_mib +command:
    #!/usr/bin/env bash
    set -euo pipefail
    memory_mib=$1
    shift
    (( $# > 0 )) || { printf 'guard requires a command\n' >&2; exit 2; }
    requested_bytes=$((memory_mib * 1024 * 1024))
    cgroup_path=''
    while IFS=: read -r hierarchy controllers path; do
        if [[ $hierarchy == 0 && -z $controllers ]]; then cgroup_path=$path; break; fi
    done < /proc/self/cgroup
    [[ -n $cgroup_path ]] || { printf 'guard cannot locate unified cgroup\n' >&2; exit 1; }
    cgroup_dir="/sys/fs/cgroup$cgroup_path"
    for file in memory.max memory.swap.max memory.oom.group; do
        [[ -r $cgroup_dir/$file ]] || { printf 'guard cannot read %s/%s\n' "$cgroup_path" "$file" >&2; exit 1; }
    done
    memory_max=$(<"$cgroup_dir/memory.max")
    swap_max=$(<"$cgroup_dir/memory.swap.max")
    oom_group=$(<"$cgroup_dir/memory.oom.group")
    [[ $memory_max =~ ^[0-9]+$ ]] && (( memory_max <= requested_bytes )) || {
        printf 'guard rejected memory.max=%s (requested at most %s)\n' "$memory_max" "$requested_bytes" >&2
        exit 1
    }
    [[ $swap_max == 0 ]] || { printf 'guard rejected memory.swap.max=%s\n' "$swap_max" >&2; exit 1; }
    [[ $oom_group == 1 ]] || { printf 'guard rejected memory.oom.group=%s\n' "$oom_group" >&2; exit 1; }
    cache_dir="$HOME/.cache/blend-verification"
    mkdir -p "$cache_dir"
    exec {lock_fd}>"$cache_dir/run.lock"
    flock -n "$lock_fd" || { printf 'another bounded verification workload is running\n' >&2; exit 75; }
    if [[ -n ${VERIFY_RUN_DIR:-} ]]; then
        printf '%s\n' "$cgroup_path" > "$VERIFY_RUN_DIR/cgroup.path"
        cat "$cgroup_dir/memory.events" > "$VERIFY_RUN_DIR/memory.events"
    fi
    export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1
    printf 'guard: cgroup=%s memory.max=%s memory.swap.max=%s memory.oom.group=%s tasks.max=256 cpu.quota=200%%\n' \
        "$cgroup_path" "$memory_max" "$swap_max" "$oom_group" >&2
    exec "$@"

# Build all production WASM contracts through the existing Makefile.
build:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 3600 make build

# Run the existing production and integration test entrypoint.
test:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 3600 make test

# Run standalone fuzz-driver regression tests under the verification guard.
test-fuzz:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 1800 \
        cargo +nightly-2025-11-25 test --manifest-path "$root/test-suites/fuzz/Cargo.toml" --offline --locked --lib

# Emit a finite fuzz-target or Kani-harness matrix from the registry.
verify-list kind selection="all":
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    kind=$1
    selection=$2
    registry="$root/verification/targets.json"
    [[ -s $registry ]] || { printf 'missing verification registry: %s\n' "$registry" >&2; exit 1; }
    [[ -n $selection ]] || { printf 'selection must not be empty\n' >&2; exit 2; }
    jq -e '
        .version == 1 and
        (.fuzz | type == "array" and length > 0) and
        (.kani | type == "array" and length > 0) and
        ([.fuzz[].target] | length == (unique | length)) and
        ([.kani[].harness] | length == (unique | length)) and
        (all(.fuzz[]; (.target | test("^fuzz_[a-z0-9_]+$")) and (.contracts | length > 0) and (.entrypoints | length > 0))) and
        (all(.kani[]; (.group == "pool" or .group == "backstop" or .group == "factory") and (.harness | test("^kani_proofs::[a-z0-9_:]+$"))))
    ' "$registry" >/dev/null || { printf 'verification registry is invalid\n' >&2; exit 1; }
    case $kind in
        fuzz)
            if [[ $selection == all ]]; then
                jq -ce '{include:[.fuzz[] | {target:.target}]}' "$registry"
            else
                jq -ce --arg selection "$selection" '
                    [.fuzz[] | select(.target == $selection) | {target:.target}] as $selected |
                    if ($selected | length) == 1 then {include:$selected} else error("unknown fuzz target: " + $selection) end
                ' "$registry"
            fi
            ;;
        kani)
            if [[ $selection == all ]]; then
                jq -ce '{include:[.kani[] | {group:.group,harness:.harness}]}' "$registry"
            else
                jq -ce --arg selection "$selection" '
                    [.kani[] | select(.group == $selection or .harness == $selection) | {group:.group,harness:.harness}] as $selected |
                    if ($selected | length) > 0 then {include:$selected} else error("unknown Kani group or harness: " + $selection) end
                ' "$registry"
            fi
            ;;
        *) printf 'kind must be fuzz or kani\n' >&2; exit 2 ;;
    esac

# Run selected libFuzzer targets sequentially after exact registry discovery.
fuzz target="all" seconds="60" seed="1":
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    target=$1
    seconds=$2
    seed=$3
    [[ $seconds =~ ^[0-9]+$ ]] && (( seconds >= 1 && seconds <= 1800 )) || { printf 'seconds must be an integer in 1..=1800\n' >&2; exit 2; }
    [[ $seed =~ ^[0-9]+$ ]] && (( seed >= 1 && seed <= 4294967295 )) || { printf 'seed must be an integer in 1..=4294967295\n' >&2; exit 2; }
    matrix=$("$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-list fuzz "$target")
    "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 300 \
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" _verify-fuzz-registry
    while IFS= read -r selected; do
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 1800 \
            "$just_bin" --justfile "$root/justfile" --working-directory "$root" _build-fuzz-target "$selected"
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run "$((seconds + 60))" \
            "$just_bin" --justfile "$root/justfile" --working-directory "$root" _run-fuzz-target "$selected" "$seconds" "$seed"
    done < <(jq -r '.include[].target' <<<"$matrix")

# Replay committed seeds or one exact input through the instrumented and logical drivers.
fuzz-replay target="all" mode="both" input="":
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    target=$1
    mode=$2
    input=$3
    [[ $mode == native || $mode == wasm || $mode == both ]] || { printf 'mode must be native, wasm, or both\n' >&2; exit 2; }
    if [[ -n $input && $target == all ]]; then printf 'an input file requires one target\n' >&2; exit 2; fi
    if [[ -n $input ]]; then
        [[ -f $input ]] || { printf 'input does not exist: %s\n' "$input" >&2; exit 2; }
        input=$(realpath "$input")
    fi
    matrix=$("$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-list fuzz "$target")
    "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 300 \
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" _verify-fuzz-registry
    while IFS= read -r selected; do
        if [[ -n $input ]]; then
            "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 1800 \
                "$just_bin" --justfile "$root/justfile" --working-directory "$root" _build-fuzz-target "$selected"
            "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 600 \
                "$just_bin" --justfile "$root/justfile" --working-directory "$root" _run-fuzz-input "$selected" "$input"
        fi
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 600 \
            "$just_bin" --justfile "$root/justfile" --working-directory "$root" _run-replay "$selected" "$mode" "$input"
    done < <(jq -r '.include[].target' <<<"$matrix")

# Run selected exact Kani harnesses sequentially after versioned discovery.
kani group="all" harness="":
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    group=$1
    harness=$2
    selection=$group
    if [[ -n $harness ]]; then
        [[ $group == all || $group == pool || $group == backstop || $group == factory ]] || { printf 'unknown Kani group: %s\n' "$group" >&2; exit 2; }
        selection=$harness
    fi
    matrix=$("$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-list kani "$selection")
    if [[ -n $harness ]]; then
        actual_group=$(jq -r '.include[0].group' <<<"$matrix")
        [[ $group == all || $group == "$actual_group" ]] || { printf 'harness does not belong to group %s\n' "$group" >&2; exit 2; }
    fi
    lock_before=$(sha256sum "$root/Cargo.lock" | cut -d' ' -f1)
    "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 300 \
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" _verify-kani-registry
    while IFS= read -r selected; do
        safe_name=${selected//::/_}
        result="$root/target/verification/kani-${safe_name}.json"
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 660 \
            "$just_bin" --justfile "$root/justfile" --working-directory "$root" _run-kani-harness "$selected" "$result"
    done < <(jq -r '.include[].harness' <<<"$matrix")
    lock_after=$(sha256sum "$root/Cargo.lock" | cut -d' ' -f1)
    [[ $lock_before == "$lock_after" ]] || { printf 'Cargo.lock changed during Kani execution\n' >&2; exit 1; }

[private]
_verification-fetch:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    just_bin='{{ just }}'
    "$just_bin" --justfile "$root/justfile" --working-directory "$root" verify-run 3600 \
        "$just_bin" --justfile "$root/justfile" --working-directory "$root" _fetch-locked-dependencies

[private]
_fetch-locked-dependencies:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    before_root=$(sha256sum "$root/Cargo.lock" | cut -d' ' -f1)
    before_fuzz=$(sha256sum "$root/test-suites/fuzz/Cargo.lock" | cut -d' ' -f1)
    cargo +1.81 fetch --locked --manifest-path "$root/Cargo.toml"
    cargo +nightly-2025-11-25 fetch --locked --manifest-path "$root/test-suites/fuzz/Cargo.toml"
    after_root=$(sha256sum "$root/Cargo.lock" | cut -d' ' -f1)
    after_fuzz=$(sha256sum "$root/test-suites/fuzz/Cargo.lock" | cut -d' ' -f1)
    [[ $before_root == "$after_root" && $before_fuzz == "$after_fuzz" ]] || { printf 'lockfile changed during dependency fetch\n' >&2; exit 1; }

[private]
_verify-fuzz-registry:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    [[ $(cargo +nightly-2025-11-25 fuzz --version) == 'cargo-fuzz 0.13.2' ]] || { printf 'cargo-fuzz 0.13.2 is required\n' >&2; exit 1; }
    expected=$(jq -r '.fuzz[].target' "$root/verification/targets.json" | sort)
    actual=$(cd "$root/test-suites" && cargo +nightly-2025-11-25 fuzz list --fuzz-dir fuzz | sed '/^[[:space:]]*$/d' | sort)
    [[ -n $actual && $expected == "$actual" ]] || {
        printf 'fuzz registry and cargo fuzz list differ\nexpected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
        exit 1
    }

[private]
_build-fuzz-target target:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    target=$1
    jq -e --arg target "$target" 'any(.fuzz[]; .target == $target)' "$root/verification/targets.json" >/dev/null || exit 2
    mkdir -p "$root/test-suites/fuzz/corpus/$target" "$root/test-suites/fuzz/artifacts/$target" "$root/target/verification/fuzz-bin"
    before=$(sha256sum "$root/test-suites/fuzz/Cargo.lock" | cut -d' ' -f1)
    cd "$root/test-suites"
    CARGO_NET_OFFLINE=true CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true \
        cargo +nightly-2025-11-25 fuzz build --fuzz-dir fuzz "$target" --sanitizer address
    metadata=$(cargo +nightly-2025-11-25 metadata --offline --locked --format-version 1 --manifest-path "$root/test-suites/fuzz/Cargo.toml")
    target_dir=$(jq -r '.target_directory' <<<"$metadata")
    mapfile -t candidates < <(find "$target_dir" -type f -name "$target" -perm -u+x -print)
    (( ${#candidates[@]} == 1 )) || { printf 'expected one built fuzzer for %s, found %s\n' "$target" "${#candidates[@]}" >&2; printf '%s\n' "${candidates[@]}" >&2; exit 1; }
    printf '%s\n' "$(realpath "${candidates[0]}")" > "$root/target/verification/fuzz-bin/$target.path"
    after=$(sha256sum "$root/test-suites/fuzz/Cargo.lock" | cut -d' ' -f1)
    [[ $before == "$after" ]] || { printf 'fuzz lockfile changed during build\n' >&2; exit 1; }

[private]
_run-fuzz-target target seconds seed:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    target=$1
    seconds=$2
    seed=$3
    binary=$(<"$root/target/verification/fuzz-bin/$target.path")
    [[ -x $binary ]] || { printf 'missing fuzz binary for %s\n' "$target" >&2; exit 1; }
    mkdir -p "$root/test-suites/fuzz/corpus/$target" "$root/test-suites/fuzz/artifacts/$target"
    printf 'libFuzzer: target=%s -max_len=65\n' "$target" >&2
    exec "$binary" "$root/test-suites/fuzz/corpus/$target" "$root/test-suites/fuzz/seeds/$target" \
        "-artifact_prefix=$root/test-suites/fuzz/artifacts/$target/" "-max_total_time=$seconds" -timeout=10 \
        -rss_limit_mb=2048 -max_len=65 "-seed=$seed" -print_final_stats=1

[private]
_run-fuzz-input target input:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    target=$1
    input=$2
    binary=$(<"$root/target/verification/fuzz-bin/$target.path")
    [[ -x $binary && -f $input ]] || exit 1
    mkdir -p "$root/test-suites/fuzz/artifacts/$target"
    printf 'libFuzzer: target=%s -max_len=65\n' "$target" >&2
    exec "$binary" "$input" "-artifact_prefix=$root/test-suites/fuzz/artifacts/$target/" \
        -runs=1 -timeout=10 -rss_limit_mb=2048 -max_len=65 -print_final_stats=1

[private]
_run-replay target mode input:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    target=$1
    mode=$2
    input=$3
    args=(--target "$target" --mode "$mode")
    if [[ -n $input ]]; then args+=(--input "$input"); else args+=(--seeds); fi
    cd "$root/test-suites/fuzz"
    CARGO_NET_OFFLINE=true cargo +nightly-2025-11-25 run --offline --locked --release --example replay -- "${args[@]}"

[private]
_verify-kani-registry:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    [[ $(cargo kani --version) == *'0.68.0'* ]] || { printf 'Kani 0.68.0 is required\n' >&2; exit 1; }
    before=$(sha256sum "$root/Cargo.lock" | cut -d' ' -f1)
    mkdir -p "$root/target/verification"
    cd "$root/target/verification"
    CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$root/target/verification/kani-build" \
        cargo kani -p blend-contract-kernel --lib --manifest-path "$root/Cargo.toml" list --format json
    [[ -s kani-list.json ]] || { printf 'Kani did not produce kani-list.json\n' >&2; exit 1; }
    jq -e '."file-version" == "0.1"' kani-list.json >/dev/null || { printf 'unsupported Kani list schema; update the parser deliberately\n' >&2; exit 1; }
    actual=$(jq -r '.["standard-harnesses"] | to_entries[] | .value[]' kani-list.json | sort -u)
    expected=$(jq -r '.kani[].harness' "$root/verification/targets.json" | sort -u)
    [[ -n $actual && $actual == "$expected" ]] || { printf 'Kani registry differs from discovery\nexpected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2; exit 1; }
    jq -e '
        [.kani[] | select(.partition_set == "threshold_agreement") | .partition] | sort_by(.min) as $partitions |
        (($partitions | length) > 0) and
        (all($partitions[];
            .axis == "blnd_whole_seed" and
            ((.min | type) == "number") and (.min == (.min | floor)) and
            ((.max | type) == "number") and (.max == (.max | floor)) and
            .min >= 0 and .min <= .max and .max <= 255)) and
        ($partitions[0].min == 0) and
        ($partitions[-1].max == 255) and
        (all(range(1; ($partitions | length)); . as $index |
            $partitions[$index].min == ($partitions[$index - 1].max + 1)))
    ' "$root/verification/targets.json" >/dev/null || {
        printf 'threshold agreement partitions must be nonempty integer intervals covering the u8 BLND seed axis exactly\n' >&2
        exit 1
    }
    jq -e '
        [.kani[] | select(.partition_set == "threshold_monotonicity") | .partition] as $partitions |
        (($partitions | length) > 0) and
        (all($partitions[];
            .axis == "blnd_whole_seed" and
            (.direction == "blnd" or .direction == "usdc") and
            ((.min | type) == "number") and (.min == (.min | floor)) and
            ((.max | type) == "number") and (.max == (.max | floor)) and
            .min >= 0 and .min <= .max and .max <= 255)) and
        (all(["blnd", "usdc"][]; . as $direction |
            ([$partitions[] | select(.direction == $direction)] | sort_by(.min)) as $tiles |
            (($tiles | length) > 0) and
            ($tiles[0].min == 0) and
            ($tiles[-1].max == 255) and
            (all(range(1; ($tiles | length)); . as $index |
                $tiles[$index].min == ($tiles[$index - 1].max + 1)))))
    ' "$root/verification/targets.json" >/dev/null || {
        printf 'threshold monotonicity partitions must be nonempty integer intervals covering both directions exactly\n' >&2
        exit 1
    }
    after=$(sha256sum "$root/Cargo.lock" | cut -d' ' -f1)
    [[ $before == "$after" ]] || { printf 'Cargo.lock changed during Kani listing\n' >&2; exit 1; }

[private]
_run-kani-harness harness result:
    #!/usr/bin/env bash
    set -euo pipefail
    root='{{ root }}'
    harness=$1
    result=$2
    jq -e --arg harness "$harness" 'any(.kani[]; .harness == $harness)' "$root/verification/targets.json" >/dev/null || exit 2
    mkdir -p "$(dirname "$result")" "$root/target/verification/kani-build"
    CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$root/target/verification/kani-build" \
        cargo kani -p blend-contract-kernel --lib --harness "$harness" --exact --jobs=1 --output-format=terse \
        -Z unstable-options --harness-timeout 600s --export-json "$result"
    [[ -s $result ]] || { printf 'Kani result is missing: %s\n' "$result" >&2; exit 1; }
    jq -e --arg harness "$harness" --slurpfile registry "$root/verification/targets.json" '
        select(.metadata.version == "1.0" and .metadata.kani_version == "0.68.0") |
        (.verification_results.results | map(select(.harness_id == $harness))) as $results |
        (.property_details | map(select(.harness_id == $harness))) as $properties |
        ($registry[0].kani | map(select(.harness == $harness)) | .[0].covers | sort) as $expected_covers |
        (.verification_results.summary.status == "completed") and
        (.verification_results.summary.total_harnesses == 1) and
        (.verification_results.summary.executed == 1) and
        (.verification_results.summary.successful == 1) and
        (.verification_results.summary.failed == 0) and
        (($results | length) == 1) and
        (($results[0].status | ascii_downcase) == "success") and
        (($properties | length) == 1) and
        ($properties[0].property_details.failed == 0) and
        ($properties[0].property_details.undetermined == 0) and
        ($properties[0].property_details.solver_error == 0) and
        ($properties[0].property_details.unsatisfiable == 0) and
        ($properties[0].property_details.satisfied == ($expected_covers | length)) and
        (([$results[0].checks[] | select(.category == "cover") | .description] | sort) == $expected_covers) and
        (all($results[0].checks[] | select(.category == "cover"); .status == "Satisfied"))
    ' "$result" >/dev/null || { printf 'Kani result or cover obligations failed for exact harness %s\n' "$harness" >&2; exit 1; }
