import { Header } from "@/components/Chrome";
import { Hero } from "@/components/Hero";
import { Backends } from "@/components/Backends";
import { Proof } from "@/components/Proof";
import { Replay } from "@/components/Replay";
import { Boundary } from "@/components/Boundary";
import { Footer } from "@/components/Footer";

export default function Page() {
  return (
    <>
      <Header />
      <main>
        <Hero />
        <Backends />
        <Proof />
        <Replay />
        <Boundary />
      </main>
      <Footer />
    </>
  );
}
