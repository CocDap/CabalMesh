/** The footer links. */
export default function Closing() {
  return (
    <footer className="footer closing footer--dark ground-inverse" id="contact">
      <div className="wrap">
        <div className="closing__links">
          <div className="grid">
            <div className="span-4 stack-4">
              <a className="nav__logo" href="#top">
                <img src="brand/mark.png" alt="" width="20" height="24" loading="lazy" />
                CabalMesh
              </a>
              <p className="footer__lede">Phones that trade over a mesh and settle on NEAR.</p>
            </div>
            <div className="span-2 start-7 footer__col">
              <h3>A trade</h3>
              <ul>
                <li>
                  <a href="#app">The app</a>
                </li>
                <li>
                  <a href="#how">Three steps</a>
                </li>
                <li>
                  <a href="#sequence">Sequence</a>
                </li>
              </ul>
            </div>
            <div className="span-2 footer__col">
              <h3>Project</h3>
              <ul>
                <li>
                  <a href="#why">NEAR Intents</a>
                </li>
                <li>
                  <a href="#roadmap">Roadmap</a>
                </li>
              </ul>
            </div>
            <div className="span-2 footer__col">
              <h3>Code</h3>
              <ul>
                <li>
                  <a href="https://github.com/CocDap/CabalMesh">GitHub</a>
                </li>
              </ul>
            </div>
          </div>
          <div className="footer__legal">
            <span>© 2026 CabalMesh. Built on NEAR.</span>
          </div>
        </div>
      </div>
    </footer>
  );
}
