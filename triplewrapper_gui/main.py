"""Entry point for TripleWrapper GUI."""
import sys
from .application import TripleWrapperApplication

def main() -> int:
    app = TripleWrapperApplication(
        application_id="com.github.triplewrapper.App",
        resourcebasepath="/com/github/triplewrapper/",
    )
    return app.run(sys.argv)

if __name__ == "__main__":
    sys.exit(main())