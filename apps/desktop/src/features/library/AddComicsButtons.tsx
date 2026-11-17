import { FolderPlus, Plus } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { Menu } from "../../components/Menu";
import { useCompact } from "../../app/platform";
import { useImport } from "./import-context";

export function AddComicsButtons({ compact }: { compact?: boolean }) {
  const { pickAndImport } = useImport();
  // No celular os dois botões não cabem ao lado do título: um "+" abre a escolha numa folha.
  const phone = useCompact();
  if (phone) {
    return (
      <Menu
        label="Adicionar quadrinhos"
        items={[
          { label: "Escolher uma pasta", icon: FolderPlus, onSelect: () => void pickAndImport("folder") },
          { label: "Escolher arquivos", icon: Plus, onSelect: () => void pickAndImport("files") },
        ]}
        trigger={(p) => <IconButton {...p} icon={Plus} label="Adicionar quadrinhos" data-tour="add-comics" />}
      />
    );
  }
  return (
    <>
      <Button size={compact ? "sm" : "md"} icon={FolderPlus} onClick={() => void pickAndImport("folder")}>
        Adicionar pasta
      </Button>
      <Button size={compact ? "sm" : "md"} variant="primary" icon={Plus} onClick={() => void pickAndImport("files")} data-tour="add-comics">
        Adicionar quadrinhos
      </Button>
    </>
  );
}
