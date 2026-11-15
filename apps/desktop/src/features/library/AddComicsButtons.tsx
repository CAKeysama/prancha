import { FolderPlus, Plus } from "lucide-react";
import { Button } from "../../components/Button";
import { useImport } from "./import-context";

export function AddComicsButtons({ compact }: { compact?: boolean }) {
  const { pickAndImport } = useImport();
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
