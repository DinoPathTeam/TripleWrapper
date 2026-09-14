"""Entry point for TripleWrapper GUI."""
import sys

from .application import TripleWrapperApplication


def main() -> int:
    app = TripleWrapperApplication(
        application_id="io.github.dinopathtream.TripleWrapper",
        resourcebasepath="/io/github/dinopathtream/TripleWrapper/",
    )
    return int(app.run(sys.argv))

if __name__ == "__main__":
    sys.exit(main())