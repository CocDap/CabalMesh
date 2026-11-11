/** The app in a window frame, replaying the $3.78 run: the intent chat beside its agent log. */
export default function AppFrame() {
  return (
    <section className="section" id="app" aria-labelledby="app-h">
      <div className="wrap">
        <div className="grid grid--top">
          <h2 id="app-h" className="span-5">Every intent beside its agent's log</h2>
        </div>
        <div className="ui app fill" role="img" aria-label="The intents list beside the open sale of 0.1 wNEAR, where the agents agreed at $3.78 and wait for the owner to confirm" data-ui-pause-host="">
          <div className="app__pane app__pane--list">
            <div className="ui ui-panel ui-win" data-ui="window" data-open="Open" data-work="Negotiating" data-done="Settled">
              <div className="ui-win__bar">
                <span className="ui-dots" aria-hidden="true">
                  <i />
                  <i />
                  <i />
                </span>
                {" "}
                <span className="ui-win__crumb">
                  {"CabalMesh "}
                  <i>/</i>
                  {" "}
                  <b>Intents</b>
                </span>
                {" "}
                <span className="ui-spacer" />
                {" "}
                <span className="ui-win__search" aria-hidden="true">
                  <svg className="ui-icon" viewBox="0 0 16 16">
                    <circle cx="7" cy="7" r="4.5" />
                    <path d="m10.5 10.5 3 3" />
                  </svg>
                  <span className="ui-win__ph">Search</span>
                  <span className="ui-kbd">/</span>
                </span>
              </div>
              <div className="ui-win__body">
                <nav className="ui-win__side" aria-hidden="true">
                  <span className="ui-win__nav is-on">
                    <svg className="ui-icon" viewBox="0 0 16 16">
                      <path d="M2.5 9h3l1 1.5h3l1-1.5h3" />
                      <path d="M2.5 9 4 3.5h8L13.5 9v3.5h-11z" />
                    </svg>
                    Open
                    <em>4</em>
                  </span>
                  {" "}
                  <span className="ui-win__nav">
                    <svg className="ui-icon" viewBox="0 0 16 16">
                      <circle cx="8" cy="6" r="2.5" />
                      <path d="M3.5 13.5c.6-2.3 2.4-3.5 4.5-3.5s3.9 1.2 4.5 3.5" />
                    </svg>
                    Mine
                    <em>2</em>
                  </span>
                  {" "}
                  <span className="ui-win__nav">
                    <svg className="ui-icon" viewBox="0 0 16 16">
                      <circle cx="8" cy="8" r="5.5" />
                      <path d="M8 5v3l2 1.5" />
                    </svg>
                    Signing
                    <em>1</em>
                  </span>
                  {" "}
                  <span className="ui-win__nav">
                    <svg className="ui-icon" viewBox="0 0 16 16">
                      <path d="m3.5 8.4 2.8 2.8 6.2-6.4" />
                    </svg>
                    Settled
                  </span>
                  {" "}
                  <span className="ui-win__label">Assets</span>
                  {" "}
                  <span className="ui-win__nav">
                    <span className="ui-win__hash">#</span>
                    wNEAR
                  </span>
                  {" "}
                  <span className="ui-win__nav">
                    <span className="ui-win__hash">#</span>
                    USDC
                  </span>
                </nav>
                <div className="ui-win__main">
                  <div className="ui-win__cols" aria-hidden="true">
                    <span>Peer</span>
                    <span>Intent</span>
                    <span>Status</span>
                    <span>Age</span>
                  </div>
                  <ul className="ui-win__rows">
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="1">ME</span>
                      <span className="ui-win__who">This phone</span>
                      <span className="ui-win__sub">Sell 0.1 wNEAR · shark mode</span>
                      <span className="ui-win__st is-you" data-st="">Confirm</span>
                      <span className="ui-win__age">30s</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="3">BL</span>
                      <span className="ui-win__who">Phone B · BLE</span>
                      <span className="ui-win__sub">Buy 0.1 wNEAR · under $4</span>
                      <span className="ui-win__st" data-st="">Open</span>
                      <span className="ui-win__age">1m</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="4">P2</span>
                      <span className="ui-win__who">peer 12D3…x7q</span>
                      <span className="ui-win__sub">Sell 0.2 wNEAR · any price</span>
                      <span className="ui-win__st" data-st="">Open</span>
                      <span className="ui-win__age">3m</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="2">BL</span>
                      <span className="ui-win__who">Phone C · BLE</span>
                      <span className="ui-win__sub">Buy 0.5 USDC · over $0.99</span>
                      <span className="ui-win__st" data-st="">Open</span>
                      <span className="ui-win__age">4m</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="5">P4</span>
                      <span className="ui-win__who">peer 12D3…m2k</span>
                      <span className="ui-win__sub">Sell 0.1 wNEAR · above $3.70</span>
                      <span className="ui-win__st" data-st="">Open</span>
                      <span className="ui-win__age">6m</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="1">ME</span>
                      <span className="ui-win__who">This phone</span>
                      <span className="ui-win__sub">Buy 0.1 wNEAR · settled $3.70</span>
                      <span className="ui-win__st is-done" data-st="">Settled</span>
                      <span className="ui-win__age">12m</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="3">BL</span>
                      <span className="ui-win__who">Phone B · BLE</span>
                      <span className="ui-win__sub">Sell 0.1 wNEAR · settled $3.78</span>
                      <span className="ui-win__st is-done" data-st="">Settled</span>
                      <span className="ui-win__age">20m</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="4">P2</span>
                      <span className="ui-win__who">peer 12D3…x7q</span>
                      <span className="ui-win__sub">Buy 0.1 wNEAR · settled $3.74</span>
                      <span className="ui-win__st is-done" data-st="">Settled</span>
                      <span className="ui-win__age">41m</span>
                    </li>
                    <li className="ui-win__row" data-row="">
                      <span className="ui-avatar" data-tone="2">BL</span>
                      <span className="ui-win__who">Phone C · BLE</span>
                      <span className="ui-win__sub">Sell 0.5 USDC · no deal</span>
                      <span className="ui-win__st is-done" data-st="">Expired</span>
                      <span className="ui-win__age">1h</span>
                    </li>
                  </ul>
                </div>
              </div>
            </div>
          </div>
          <div className="app__pane app__pane--open" data-ui-loop="">
            <div className="ui ui-panel ui-chat" data-ui="chat">
              <div className="ui-head">
                <span className="ui-agent" aria-hidden="true" />
                <div className="ui-title">Agent · Sell 0.1 wNEAR</div>
                <span className="ui-chat__online">Negotiating</span>
                {" "}
                <span className="ui-spacer" />
              </div>
              <div className="ui-chat__body">
                <div className="ui-chat__day">Round 2 of 3</div>
                <div className="ui-msg ui-msg--me" data-chat-q="">
                  <span className="ui-msg__text">Sell 0.1 wNEAR. Any price, shark mode.</span>
                </div>
                <div className="ui-msg ui-msg--bot" data-chat-a="">
                  <span className="ui-agent ui-msg__av" aria-hidden="true" />
                  {" "}
                  <span className="ui-msg__col">
                    <span className="ui-chat__answer">
                      {"I offered $4.15, the peer said $3.70. I came down to $3.88 and the peer said $3.78. "}
                      <b>Agents agreed at $3.78.</b>
                      {" You receive 0.378 USDC. Confirm to sign?"}
                    </span>
                    {" "}
                    <span className="ui-chat__srcs">
                      <span className="ui-chat__src">
                        <svg className="ui-icon" viewBox="0 0 16 16">
                          <path d="M4 2.5h5l3 3v8H4z" />
                          <path d="M9 2.5v3h3" />
                        </svg>
                        Market price
                      </span>
                      {" "}
                      <span className="ui-chat__src">
                        <svg className="ui-icon" viewBox="0 0 16 16">
                          <circle cx="8" cy="8" r="5.5" />
                          <path d="M8 5v3l2 1.5" />
                        </svg>
                        Mesh offers
                      </span>
                    </span>
                  </span>
                </div>
                <div className="ui-msg ui-msg--me">
                  <span className="ui-msg__text">Confirm. Gateway fee 0.01 USDC is fine.</span>
                </div>
              </div>
              <div className="ui-chat__input">
                <span className="ui-chat__ph">Write an intent</span>
                {" "}
                <span className="ui-chat__send" aria-hidden="true">
                  <svg viewBox="0 0 16 16" width="14" height="14">
                    <path d="M8 13V3.5M3.8 7.5 8 3.3l4.2 4.2" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
