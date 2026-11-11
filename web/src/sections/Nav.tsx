/** Top bar: the mark, the section links and the "How it works" menu. */
export default function Nav() {
  return (
    <header className="nav">
      <div className="wrap nav__bar">
        <a className="skip-link" href="#top">Skip to content</a>
        {" "}
        <a className="nav__logo" href="#top">
          <img src="brand/mark.png" alt="" width="20" height="24" />
          CabalMesh
        </a>
        <nav className="nav__links" aria-label="Main">
          <div className="nav__item">
            <a className="nav__link" href="#how">
              {"How it works "}
              <svg className="chev" viewBox="0 0 12 12" aria-hidden="true">
                <path d="M3 4.5l3 3 3-3" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </a>
            <div className="nav__menu">
              <div className="wrap grid">
                <div className="menu-col span-3">
                  <p className="t-label">A trade</p>
                  <a href="#app">
                    The app
                    <span>Intents and the agent's log</span>
                  </a>
                  {" "}
                  <a href="#how">
                    Three steps
                    <span>Bid, sign, settle on NEAR</span>
                  </a>
                  {" "}
                  <a href="#sequence">
                    The sequence
                    <span>Every message of one trade</span>
                  </a>
                </div>
                <div className="menu-col span-3">
                  <p className="t-label">The project</p>
                  <a href="#why">
                    NEAR Intents
                    <span>Why it fits a mesh</span>
                  </a>
                  {" "}
                  <a href="#roadmap">
                    Roadmap
                    <span>What runs, what comes next</span>
                  </a>
                </div>
                <a className="menu-card span-3 start-10" href="#how">
                  <div className="panel">
                    <img className="frame__img" src="brand/shots/A-agreed.jpg" alt="The CabalMesh intent screen with the agents agreed at $3.78" style={{ objectPosition: "50% 78%" }} width="720" height="1598" loading="lazy" />
                  </div>
                  <span className="t-card">Agents agreed at $3.78</span>
                  {" "}
                  <span className="t-small muted">Round 2 of 3, on two phones</span>
                </a>
              </div>
            </div>
          </div>
          <div className="nav__item">
            <a className="nav__link" href="#sequence">Sequence</a>
          </div>
          <div className="nav__item">
            <a className="nav__link" href="#why">NEAR Intents</a>
          </div>
          <div className="nav__item">
            <a className="nav__link" href="#roadmap">Roadmap</a>
          </div>
        </nav>
        <button className="nav__toggle" type="button" aria-expanded="false" aria-label="Menu">
          <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
            <path d="M2 5h14M2 13h14" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
          </svg>
        </button>
      </div>
    </header>
  );
}
