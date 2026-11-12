import SequenceDiagram from '../scenes/SequenceDiagram';

/** One trade message by message, from the "How CabalMesh Trades" sequence ($3.70 demo run). */
export default function Sequence() {
  return (
    <section className="section" id="sequence" aria-labelledby="seq-h">
      <div className="wrap">
        <div className="grid grid--top">
          <h2 id="seq-h" className="span-5">One crypto trade, message by message</h2>
          <p className="span-6 start-7 muted">
            How it works: two phones, one gateway and the NEAR verifier. Prices stay on each phone, and the matched pair reaches the chain once.
          </p>
        </div>
        <figure className="seq-frame">
          <SequenceDiagram />
        </figure>
        <ol className="seq-steps grid">
          <li className="span-2">
            <b>Discover</b>
            <span>The phones find each other over Bluetooth.</span>
          </li>
          <li className="span-2">
            <b>Intent</b>
            <span>Asset, side and size go out, never the price.</span>
          </li>
          <li className="span-2">
            <b>Negotiate</b>
            <span>Bids of $3.40 and $3.55; the seller holds $3.70.</span>
          </li>
          <li className="span-2">
            <b>Sign</b>
            <span>Each phone signs its half offline.</span>
          </li>
          <li className="span-2">
            <b>Route</b>
            <span>A gateway submits the matched pair.</span>
          </li>
          <li className="span-2">
            <b>Settle</b>
            <span>Both legs apply; the proof floods back.</span>
          </li>
        </ol>
      </div>
    </section>
  );
}
