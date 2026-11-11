import { WhySceneSettle, WhySceneSubmit } from '../scenes/WhyScenes';

/** Why NEAR Intents fits a mesh: the two reasons, each a title over its own small drawing. */
export default function WhyNear() {
  return (
    <section className="section" id="why" aria-labelledby="why-h">
      <div className="wrap">
        <div className="section-head">
          <h2 id="why-h">
            {"Why NEAR Intents fits a mesh. "}
            <span className="muted">Signing needs no network.</span>
          </h2>
        </div>
        <div className="grid">
          <article className="card card--open why__item span-6">
            <h3>Submit from any peer online</h3>
            <WhySceneSubmit />
          </article>
          <article className="card card--open why__item span-6">
            <h3>Settle both legs on NEAR Intents</h3>
            <WhySceneSettle />
          </article>
        </div>
      </div>
    </section>
  );
}
