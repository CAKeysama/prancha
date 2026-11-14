import { useState, type ReactNode } from "react";
import { Play } from "lucide-react";
import { Button } from "../../components/Button";
import { ChoiceCards, Slider, Switch } from "../../components/Form";
import type { InterfaceFont } from "../../domain/settings";
import { SOUND_LABELS, sound, type SoundName } from "../../services/sound";

// Controles usados na configuração inicial e em Configurações, para os dois lugares mostrarem a mesma coisa.

const FONT_OPTIONS: { value: InterfaceFont; label: string; description: string }[] = [
  { value: "hanken", label: "Hanken Grotesk", description: "Padrão. Neutra e compacta." },
  { value: "atkinson", label: "Atkinson Hyperlegible", description: "Letras bem distintas, para ler com menos esforço." },
  { value: "plex", label: "IBM Plex Sans", description: "A das versões anteriores." },
  { value: "system", label: "Sistema", description: "Segoe UI, a do Windows." },
];

/** Cartões de fonte, cada um escrito na própria fonte. */
export function FontChoice({ value, onChange }: { value: InterfaceFont; onChange: (v: InterfaceFont) => void }) {
  return (
    <ChoiceCards
      label="Fonte da interface"
      hideLabel
      value={value}
      onChange={onChange}
      className="grid-cols-2"
      options={FONT_OPTIONS.map((f) => ({
        ...f,
        attrs: { "data-font-preview": f.value },
        visual: (
          <span aria-hidden className="block text-2xl leading-none font-semibold text-fg">
            Aa <span className="font-normal text-muted">123</span>
          </span>
        ),
      }))}
    />
  );
}

const ALL_SOUNDS = Object.keys(SOUND_LABELS) as SoundName[];

/** Liga, volume e exemplos para ouvir. `anchor` embrulha cada opção (a pesquisa das Configurações aponta para elas). */
export function SoundControls({
  enabled,
  volume,
  onEnabled,
  onVolume,
  samples = ALL_SOUNDS,
  anchor = (_id, children) => children,
}: {
  enabled: boolean;
  volume: number;
  onEnabled: (v: boolean) => void;
  onVolume: (v: number) => void;
  samples?: SoundName[];
  anchor?: (id: "sons" | "volume", children: ReactNode) => ReactNode;
}) {
  // O valor arrastado aparece na hora; o salvo só muda ao soltar. Se ele mudar por fora (Restaurar padrão), o controle acompanha.
  const [draft, setDraft] = useState(volume);
  const [saved, setSaved] = useState(volume);
  if (saved !== volume) {
    setSaved(volume);
    setDraft(volume);
  }
  return (
    <div className="space-y-5">
      {anchor(
        "sons",
        <Switch
          label="Sons da interface"
          checked={enabled}
          onChange={(v) => {
            onEnabled(v);
            if (v) sound.play("confirm", { preview: true, volume: draft / 100 });
          }}
        />,
      )}
      {anchor(
        "volume",
        <div className="space-y-4">
          <Slider
            label="Volume"
            value={draft}
            disabled={!enabled}
            valueText={`${draft}%`}
            onChange={setDraft}
            onCommit={(v) => {
              if (v !== volume) onVolume(v);
              sound.play("confirm", { preview: true, volume: v / 100 });
            }}
          />
          <div>
            <span className="mb-2 block text-sm font-medium">Ouvir</span>
            <div className="flex flex-wrap gap-2">
              {samples.map((s) => (
                <Button key={s} size="sm" icon={Play} onClick={() => sound.play(s, { preview: true, volume: draft / 100 })}>
                  {SOUND_LABELS[s]}
                </Button>
              ))}
            </div>
          </div>
        </div>,
      )}
    </div>
  );
}
