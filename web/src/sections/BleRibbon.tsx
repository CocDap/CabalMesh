const base = import.meta.env.BASE_URL;

/**
 * The BLE wave ribbon: two sources whose rings run along a band and cross in the middle
 * (public/brand/ble-ribbon.webp, a 4s loop with a transparent ground). Readers who ask for reduced
 * motion get its first frame instead (ble-ribbon-still.webp).
 */
export default function BleRibbon({ className = '' }: { className?: string }) {
  return (
    <div className={`ribbon ${className}`.trim()}>
      <picture>
        <source srcSet={`${base}brand/ble-ribbon.webp`} media="(prefers-reduced-motion: no-preference)" type="image/webp" />
        <img className="ribbon__img" src={`${base}brand/ble-ribbon-still.webp`} alt="" width={540} height={540} decoding="async" />
      </picture>
    </div>
  );
}
