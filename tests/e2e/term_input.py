"""Run a command with a real terminal as its standard input, as a person at a keyboard does: `kaku approve` and
`kaku agent grant` refuse a digest or a grant that arrives through a pipe."""
import os
import pty
import subprocess


def run_tty(cmd, text="", timeout=180, env=None):
    master, slave = pty.openpty()
    try:
        p = subprocess.Popen(cmd, stdin=slave, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    finally:
        os.close(slave)
    try:
        if text:
            os.write(master, text.encode())
        out, err = p.communicate(timeout=timeout)
    finally:
        os.close(master)
    return subprocess.CompletedProcess(cmd, p.returncode, out, err)


if __name__ == "__main__":
    # for tests in other languages: `python3 term_input.py CMD ARGS...` runs CMD with a terminal as its input, typing what
    # arrives on this program's own input, and passes its output and exit status through
    import sys

    done = run_tty(sys.argv[1:], sys.stdin.read())
    sys.stdout.write(done.stdout)
    sys.stderr.write(done.stderr)
    sys.exit(done.returncode)
