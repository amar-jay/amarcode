import agents from "@/lib/agents.json";

const activeAgents = agents.filter((agent) => !agent.quarantined);

export default function AgentLogos() {
  return (
    <section
      aria-labelledby="agents-title"
      className="bg-[#f5f8fb] px-6 py-16 text-[#142d46] sm:py-24"
    >
      <div className="mx-auto max-w-6xl">
        <div className="mb-12 text-center">
          <p className="mb-3 text-lg">Your agents. One place.</p>
          <h2 id="agents-title" className="text-4xl font-light sm:text-5xl">
            Made for the way you work.
          </h2>
          <p className="mt-4 text-xl">
            Connect your coding agents through ACP.
          </p>
        </div>
        <ul className="grid grid-cols-2 gap-x-6 gap-y-9 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          {activeAgents.map((agent) => (
            <li
              key={agent.id}
              className="flex min-w-0 flex-col items-center gap-3 text-center"
            >
              <span
                aria-hidden="true"
                className="block size-8 bg-current"
                style={{
                  maskImage: `url(${agent.icon})`,
                  maskSize: "contain",
                  maskRepeat: "no-repeat",
                  maskPosition: "center",
                }}
              />
              <span className="text-lg leading-tight">{agent.name}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
